//! Linux-only, descriptor-owned native checker containment.
//!
//! This module is the narrow syscall boundary for clone3, cgroup v2, mount,
//! credential dropping, Landlock, seccomp and execve.  The parent module stays
//! safe Rust and exposes no arbitrary executable/argv surface.

#![allow(unsafe_code)]

use std::ffi::{CStr, CString};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::mem;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::time::{Duration, Instant};

use landlock::{
    AccessFs, CompatLevel, Compatible, PathBeneath, PathFd, Ruleset, RulesetAttr,
    RulesetCreatedAttr, RulesetStatus, ABI,
};
use seccompiler::{SeccompAction, SeccompFilter, TargetArch};
use sha2::{Digest, Sha256};

use super::{
    execute_with_operations, ContainedExecution, ContainmentFailure, ContainmentOperations,
    ResourceSnapshot, RunOperationId, TerminalWait, VerifiedCheckerMaterials,
};
use crate::cgroupfs_fd::{self, ExecutionLeaf};
use crate::toolchain_compatibility::VerifiedStartupToolchainCompatibility;

const OUTER_WALL: Duration = Duration::from_secs(100);
const CLEANUP_DEADLINE: Duration = Duration::from_secs(10);
const POLL_INTERVAL: Duration = Duration::from_millis(10);
const OUTPUT_LIMIT: usize = 1_048_576;
const CPU_TOTAL_USEC: u64 = 120_000_000;
const SETUP_ERROR_LIMIT: usize = 4096;
const CLONE_INTO_CGROUP_FLAG: u64 = 1_u64 << 33;

const CHECKER_PATH: &CStr = c"/usr/bin/python3.12";
const CHECKER_SCRIPT: &CStr =
    c"/usr/share/boole/native-shadow/checkers/rust-tuple-struct-project-v1/checker.py";
const TOOLCHAIN_BIN: &CStr = c"/opt/boole/native-checker-toolchain/bin";
const WORK_PATH: &CStr = c"/work";
const PROC_PATH: &CStr = c"/proc";

pub(super) fn execute(
    compatibility: &VerifiedStartupToolchainCompatibility,
    materials: VerifiedCheckerMaterials,
) -> Result<ContainedExecution, ContainmentFailure> {
    let recovery = compatibility.recovery();
    let manager = recovery.manager();
    let (service_root, _) = manager.recovery_directories();
    let identities = manager
        .instance()
        .lifetime_lock()
        .prerequisites()
        .identities();
    let mut operations = LinuxOperations {
        service_root,
        checker_uid: identities.checker_uid(),
        checker_gid: identities.checker_gid(),
        materials,
    };
    execute_with_operations(operations.materials.operation, &mut operations)
}

struct LinuxOperations<'a> {
    service_root: &'a cgroupfs_fd::CgroupDirectory,
    checker_uid: u32,
    checker_gid: u32,
    materials: VerifiedCheckerMaterials,
}

struct LinuxChild {
    pid: libc::pid_t,
    pidfd: OwnedFd,
    stdout: File,
    stderr: File,
    setup_status: File,
    started: Instant,
    output: Option<CapturedOutput>,
    reaped: bool,
}

#[derive(Default)]
struct CapturedOutput {
    stdout: CapturedStream,
    stderr: CapturedStream,
    retained_bytes: usize,
    timed_out: bool,
    overflow: bool,
}

#[derive(Default)]
struct CapturedStream {
    retained: Vec<u8>,
    total_bytes: u64,
    sha256: Sha256,
}

#[derive(Clone, Copy)]
enum OutputStream {
    Stdout,
    Stderr,
}

impl CapturedOutput {
    fn observe(&mut self, stream: OutputStream, bytes: &[u8]) -> Result<(), ContainmentFailure> {
        let added = u64::try_from(bytes.len()).map_err(|_| {
            ContainmentFailure::Platform("output byte count exceeds u64".to_string())
        })?;
        let target = match stream {
            OutputStream::Stdout => &mut self.stdout,
            OutputStream::Stderr => &mut self.stderr,
        };
        target.total_bytes = target.total_bytes.checked_add(added).ok_or_else(|| {
            ContainmentFailure::Platform("output byte count overflowed".to_string())
        })?;
        target.sha256.update(bytes);

        let remaining = OUTPUT_LIMIT.saturating_sub(self.retained_bytes);
        let retained = bytes.len().min(remaining);
        target.retained.extend_from_slice(&bytes[..retained]);
        self.retained_bytes += retained;
        if retained != bytes.len() {
            self.overflow = true;
        }
        Ok(())
    }
}

impl CapturedStream {
    fn finish(self) -> (Vec<u8>, u64, [u8; 32]) {
        (
            self.retained,
            self.total_bytes,
            self.sha256.finalize().into(),
        )
    }
}

impl ContainmentOperations for LinuxOperations<'_> {
    type Leaf = ExecutionLeaf;
    type Child = LinuxChild;
    type Output = CapturedOutput;

    fn create_leaf(
        &mut self,
        operation: &RunOperationId,
    ) -> Result<Self::Leaf, ContainmentFailure> {
        cgroupfs_fd::create_execution_leaf(self.service_root, &operation.leaf_name())
            .map_err(platform)
    }

    fn apply_fixed_limits(&mut self, leaf: &Self::Leaf) -> Result<(), ContainmentFailure> {
        cgroupfs_fd::apply_execution_leaf_limits(leaf).map_err(platform)
    }

    fn clone_child_atomically(
        &mut self,
        leaf: &Self::Leaf,
    ) -> Result<Self::Child, ContainmentFailure> {
        clone_contained_child(leaf, self.checker_uid, self.checker_gid, &self.materials)
    }

    fn wait_and_observe(
        &mut self,
        leaf: &Self::Leaf,
        child: &mut Self::Child,
    ) -> Result<(TerminalWait, ResourceSnapshot), ContainmentFailure> {
        let (wait, output) = monitor_child(leaf, child)?;
        child.output = Some(output);
        let observed = cgroupfs_fd::read_execution_resources(leaf).map_err(platform)?;
        Ok((
            wait,
            ResourceSnapshot {
                cpu_usage_usec: observed.cpu_usage_usec,
                memory_peak_bytes: observed.memory_peak_bytes,
                memory_events_low: observed.memory_events_low,
                memory_events_high: observed.memory_events_high,
                memory_events_max: observed.memory_events_max,
                memory_events_oom: observed.memory_events_oom,
                memory_events_oom_kill: observed.memory_events_oom_kill,
                memory_events_oom_group_kill: observed.memory_events_oom_group_kill,
                pids_events_max: observed.pids_events_max,
            },
        ))
    }

    fn close_child_handles(
        &mut self,
        mut child: Self::Child,
    ) -> Result<Self::Output, ContainmentFailure> {
        let mut setup = Vec::new();
        Read::by_ref(&mut child.setup_status)
            .take((SETUP_ERROR_LIMIT + 1) as u64)
            .read_to_end(&mut setup)
            .map_err(io_platform)?;
        if setup.len() > SETUP_ERROR_LIMIT {
            return Err(ContainmentFailure::Platform(
                "child setup error record exceeded its fixed ceiling".to_string(),
            ));
        }
        if !setup.is_empty() {
            return Err(ContainmentFailure::Platform(format!(
                "child setup failed: {}",
                String::from_utf8_lossy(&setup)
            )));
        }
        drop(child.pidfd);
        child.output.take().ok_or_else(|| {
            ContainmentFailure::Platform("child output was not finalized".to_string())
        })
    }

    fn terminate_tree(
        &mut self,
        leaf: &Self::Leaf,
        child: &mut Self::Child,
    ) -> Result<(), ContainmentFailure> {
        let populated_deadline = Instant::now() + CLEANUP_DEADLINE;
        if cgroupfs_fd::read_execution_leaf_event(leaf, "populated").map_err(platform)? != 0 {
            terminate_leaf(leaf)?;
        }
        if !child.reaped {
            let mut status = 0;
            // SAFETY: this consumes the one outstanding wait status for the
            // launcher's direct child after cgroup-wide termination.
            let waited = unsafe { libc::waitpid(child.pid, &mut status, 0) };
            if waited != child.pid {
                return Err(io_platform(io::Error::last_os_error()));
            }
            child.reaped = true;
        }
        cgroupfs_fd::wait_execution_leaf_event(leaf, "populated", 0, populated_deadline)
            .map_err(platform)
    }

    fn discard_child_handles(&mut self, child: Self::Child) {
        drop(child);
    }

    fn confirm_unpopulated(&mut self, leaf: &Self::Leaf) -> Result<(), ContainmentFailure> {
        let deadline = Instant::now() + CLEANUP_DEADLINE;
        cgroupfs_fd::wait_execution_leaf_event(leaf, "populated", 0, deadline).map_err(platform)?;
        cgroupfs_fd::verify_execution_leaf_ids_empty(leaf).map_err(platform)
    }

    fn remove_leaf(&mut self, leaf: Self::Leaf) -> Result<(), ContainmentFailure> {
        cgroupfs_fd::remove_execution_leaf(self.service_root, leaf).map_err(platform)
    }

    fn finish_report(
        &mut self,
        wait: TerminalWait,
        resources: ResourceSnapshot,
        output: Self::Output,
    ) -> ContainedExecution {
        let (stdout, stdout_bytes, stdout_sha256) = output.stdout.finish();
        let (stderr, stderr_bytes, stderr_sha256) = output.stderr.finish();
        ContainedExecution {
            wait,
            resources,
            stdout,
            stderr,
            stdout_bytes,
            stderr_bytes,
            stdout_sha256,
            stderr_sha256,
            timed_out: output.timed_out,
            output_overflow: output.overflow,
        }
    }
}

fn clone_contained_child(
    leaf: &ExecutionLeaf,
    checker_uid: u32,
    checker_gid: u32,
    materials: &VerifiedCheckerMaterials,
) -> Result<LinuxChild, ContainmentFailure> {
    let task = sealed_memfd(c"boole-native-task", &materials.task)?;
    let anchor = sealed_memfd(c"boole-native-anchor", &materials.anchor)?;
    let submission = sealed_memfd(c"boole-native-submission", &materials.submission)?;
    let (stdout_read, stdout_write) = pipe_cloexec_nonblocking_reader()?;
    let (stderr_read, stderr_write) = pipe_cloexec_nonblocking_reader()?;
    let (setup_read, setup_write) = pipe_cloexec()?;

    let seccomp = build_seccomp_program()?;
    let mut pidfd: libc::c_int = -1;
    let flags = CLONE_INTO_CGROUP_FLAG
        | libc::CLONE_NEWNS as u64
        | libc::CLONE_NEWPID as u64
        | libc::CLONE_PIDFD as u64;
    let mut args: libc::clone_args = unsafe { mem::zeroed() };
    args.flags = flags;
    args.pidfd = (&mut pidfd as *mut libc::c_int) as u64;
    args.exit_signal = libc::SIGCHLD as u64;
    args.cgroup = leaf.raw_fd() as u64;

    // SAFETY: `args` is the exact kernel clone3 layout, all referenced output
    // storage remains live, and no shared-VM/thread flags are present.
    let result = unsafe {
        libc::syscall(
            libc::SYS_clone3,
            &mut args as *mut libc::clone_args,
            mem::size_of::<libc::clone_args>(),
        )
    };
    if result < 0 {
        return Err(io_platform(io::Error::last_os_error()));
    }
    if result == 0 {
        drop((stdout_read, stderr_read, setup_read));
        let setup_fd = setup_write.as_raw_fd();
        let child_result = child_setup_and_exec(ChildSetup {
            checker_uid,
            checker_gid,
            task_fd: task.as_raw_fd(),
            anchor_fd: anchor.as_raw_fd(),
            submission_fd: submission.as_raw_fd(),
            stdout_fd: stdout_write.as_raw_fd(),
            stderr_fd: stderr_write.as_raw_fd(),
            setup_fd,
            seccomp,
        });
        if let Err(error) = child_result {
            write_setup_error(setup_error_fd(setup_fd), &error);
        }
        // SAFETY: this is the post-clone child and must not run destructors or
        // return into launcher control flow after setup/exec failure.
        unsafe { libc::_exit(127) }
    }

    drop((
        task,
        anchor,
        submission,
        stdout_write,
        stderr_write,
        setup_write,
    ));
    if pidfd < 0 {
        terminate_leaf(leaf)?;
        let mut status = 0;
        // SAFETY: clone3 returned this direct child, which has just been
        // terminated cgroup-wide and must be reaped before reporting setup
        // failure.
        if unsafe { libc::waitpid(result as libc::pid_t, &mut status, 0) } != result as libc::pid_t
        {
            return Err(io_platform(io::Error::last_os_error()));
        }
        cgroupfs_fd::wait_execution_leaf_event(
            leaf,
            "populated",
            0,
            Instant::now() + CLEANUP_DEADLINE,
        )
        .map_err(platform)?;
        return Err(ContainmentFailure::Platform(
            "clone3 returned without a pidfd".to_string(),
        ));
    }
    // SAFETY: clone3 returned a fresh pidfd owned by this parent.
    let pidfd = unsafe { OwnedFd::from_raw_fd(pidfd) };
    Ok(LinuxChild {
        pid: result as libc::pid_t,
        pidfd,
        stdout: File::from(stdout_read),
        stderr: File::from(stderr_read),
        setup_status: File::from(setup_read),
        started: Instant::now(),
        output: None,
        reaped: false,
    })
}

struct ChildSetup {
    checker_uid: u32,
    checker_gid: u32,
    task_fd: RawFd,
    anchor_fd: RawFd,
    submission_fd: RawFd,
    stdout_fd: RawFd,
    stderr_fd: RawFd,
    setup_fd: RawFd,
    seccomp: seccompiler::BpfProgram,
}

fn child_setup_and_exec(setup: ChildSetup) -> Result<(), String> {
    mount_private_filesystems(setup.checker_gid)?;
    materialize_authority(setup.task_fd, c"/work/task.json", setup.checker_gid)?;
    materialize_authority(setup.anchor_fd, c"/work/anchor.rs", setup.checker_gid)?;
    materialize_authority(
        setup.submission_fd,
        c"/work/submission.rs",
        setup.checker_gid,
    )?;
    create_scratch(setup.checker_gid)?;
    set_working_directory()?;
    install_stdio_and_close_fds(setup.stdout_fd, setup.stderr_fd, setup.setup_fd)?;
    drop_all_privileges(setup.checker_uid, setup.checker_gid)?;
    verify_dropped_privileges(setup.checker_uid, setup.checker_gid)?;
    apply_outer_rlimits()?;
    apply_landlock()?;
    seccompiler::apply_filter(&setup.seccomp).map_err(|error| error.to_string())?;
    exec_checker()
}

fn monitor_child(
    leaf: &ExecutionLeaf,
    child: &mut LinuxChild,
) -> Result<(TerminalWait, CapturedOutput), ContainmentFailure> {
    let mut output = CapturedOutput::default();
    let mut status = 0;
    let wait = loop {
        drain_pipe(&mut child.stdout, &mut output, OutputStream::Stdout)?;
        drain_pipe(&mut child.stderr, &mut output, OutputStream::Stderr)?;
        if output.overflow {
            if cgroupfs_fd::read_execution_leaf_event(leaf, "populated").map_err(platform)? != 0 {
                terminate_leaf(leaf)?;
            }
            // SAFETY: the direct child has either already exited or was part
            // of the just-terminated cgroup tree and must be reaped once.
            if unsafe { libc::waitpid(child.pid, &mut status, 0) } != child.pid {
                return Err(io_platform(io::Error::last_os_error()));
            }
            child.reaped = true;
            break decode_wait_status(status)?;
        }
        // SAFETY: `child.pid` is this launcher's direct child and `status` is
        // writable storage. WNOHANG preserves the monitor deadline.
        let waited = unsafe { libc::waitpid(child.pid, &mut status, libc::WNOHANG) };
        if waited == child.pid {
            child.reaped = true;
            break decode_wait_status(status)?;
        }
        if waited < 0 {
            return Err(io_platform(io::Error::last_os_error()));
        }

        let resources = cgroupfs_fd::read_execution_resources(leaf).map_err(platform)?;
        if child.started.elapsed() >= OUTER_WALL || resources.cpu_usage_usec >= CPU_TOTAL_USEC {
            output.timed_out = child.started.elapsed() >= OUTER_WALL;
            terminate_leaf(leaf)?;
            // SAFETY: after cgroup.kill the direct child must be reaped once.
            if unsafe { libc::waitpid(child.pid, &mut status, 0) } != child.pid {
                return Err(io_platform(io::Error::last_os_error()));
            }
            child.reaped = true;
            break decode_wait_status(status)?;
        }
        std::thread::sleep(POLL_INTERVAL);
    };

    if cgroupfs_fd::read_execution_leaf_event(leaf, "populated").map_err(platform)? != 0 {
        terminate_leaf(leaf)?;
    }
    let cleanup_deadline = Instant::now() + CLEANUP_DEADLINE;
    cgroupfs_fd::wait_execution_leaf_event(leaf, "populated", 0, cleanup_deadline)
        .map_err(platform)?;
    drain_pipe_to_eof(&mut child.stdout, &mut output, OutputStream::Stdout)?;
    drain_pipe_to_eof(&mut child.stderr, &mut output, OutputStream::Stderr)?;
    Ok((wait, output))
}

fn terminate_leaf(leaf: &ExecutionLeaf) -> Result<(), ContainmentFailure> {
    let deadline = Instant::now() + CLEANUP_DEADLINE;
    cgroupfs_fd::freeze_execution_leaf(leaf).map_err(platform)?;
    cgroupfs_fd::wait_execution_leaf_event(leaf, "frozen", 1, deadline).map_err(platform)?;
    cgroupfs_fd::kill_execution_leaf(leaf).map_err(platform)
}

fn decode_wait_status(status: libc::c_int) -> Result<TerminalWait, ContainmentFailure> {
    if libc::WIFEXITED(status) {
        let code = libc::WEXITSTATUS(status);
        return u8::try_from(code)
            .map(TerminalWait::Exited)
            .map_err(|_| ContainmentFailure::Platform("child exit code is invalid".to_string()));
    }
    if libc::WIFSIGNALED(status) {
        return Ok(TerminalWait::Signaled {
            signal: u8::try_from(libc::WTERMSIG(status))
                .map_err(|_| ContainmentFailure::Platform("child signal is invalid".to_string()))?,
            core_dumped: libc::WCOREDUMP(status),
        });
    }
    Err(ContainmentFailure::Platform(
        "child reached an unsupported wait state".to_string(),
    ))
}

fn drain_pipe(
    file: &mut File,
    output: &mut CapturedOutput,
    stream: OutputStream,
) -> Result<(), ContainmentFailure> {
    let mut buffer = [0_u8; 8192];
    loop {
        match file.read(&mut buffer) {
            Ok(0) => return Ok(()),
            Ok(read) => {
                output.observe(stream, &buffer[..read])?;
                // Hand control back to the monitor as soon as the shared
                // stdout+stderr ceiling is crossed.  Continuing to drain a
                // writer that never blocks would otherwise postpone the
                // cgroup-wide kill indefinitely.
                if output.overflow {
                    return Ok(());
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(io_platform(error)),
        }
    }
}

fn drain_pipe_to_eof(
    file: &mut File,
    output: &mut CapturedOutput,
    stream: OutputStream,
) -> Result<(), ContainmentFailure> {
    set_nonblocking(file.as_raw_fd(), false)?;
    drain_pipe(file, output, stream)
}

fn sealed_memfd(name: &CStr, bytes: &[u8]) -> Result<OwnedFd, ContainmentFailure> {
    // SAFETY: `name` is NUL-terminated and flags are the fixed memfd contract.
    let fd =
        unsafe { libc::memfd_create(name.as_ptr(), libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING) };
    if fd < 0 {
        return Err(io_platform(io::Error::last_os_error()));
    }
    // SAFETY: memfd_create returned a fresh descriptor.
    let mut file = unsafe { File::from_raw_fd(fd) };
    file.write_all(bytes).map_err(io_platform)?;
    file.seek(SeekFrom::Start(0)).map_err(io_platform)?;
    let seals = libc::F_SEAL_SEAL | libc::F_SEAL_SHRINK | libc::F_SEAL_GROW | libc::F_SEAL_WRITE;
    // SAFETY: F_ADD_SEALS is applied to this owned memfd only.
    if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_ADD_SEALS, seals) } != 0 {
        return Err(io_platform(io::Error::last_os_error()));
    }
    Ok(file.into())
}

fn pipe_cloexec() -> Result<(OwnedFd, OwnedFd), ContainmentFailure> {
    let mut fds = [-1; 2];
    // SAFETY: `fds` is two writable integers and ownership transfers below.
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        return Err(io_platform(io::Error::last_os_error()));
    }
    // SAFETY: pipe2 returned two fresh descriptors.
    Ok(unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) })
}

fn pipe_cloexec_nonblocking_reader() -> Result<(OwnedFd, OwnedFd), ContainmentFailure> {
    let pair = pipe_cloexec()?;
    set_nonblocking(pair.0.as_raw_fd(), true)?;
    Ok(pair)
}

fn set_nonblocking(fd: RawFd, enabled: bool) -> Result<(), ContainmentFailure> {
    // SAFETY: F_GETFL/F_SETFL operate on one live descriptor.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 {
        return Err(io_platform(io::Error::last_os_error()));
    }
    let updated = if enabled {
        flags | libc::O_NONBLOCK
    } else {
        flags & !libc::O_NONBLOCK
    };
    if unsafe { libc::fcntl(fd, libc::F_SETFL, updated) } != 0 {
        return Err(io_platform(io::Error::last_os_error()));
    }
    Ok(())
}

fn mount_private_filesystems(checker_gid: u32) -> Result<(), String> {
    mount_raw(None, c"/", None, libc::MS_REC | libc::MS_PRIVATE, None)?;
    mount_raw(
        Some(c"proc"),
        PROC_PATH,
        Some(c"proc"),
        libc::MS_NOSUID | libc::MS_NODEV | libc::MS_NOEXEC,
        None,
    )?;
    let options = CString::new(format!(
        "size=536870912,nr_inodes=8192,mode=2750,uid=0,gid={checker_gid}"
    ))
    .map_err(|_| "tmpfs mount options contain NUL".to_string())?;
    mount_raw(
        Some(c"tmpfs"),
        WORK_PATH,
        Some(c"tmpfs"),
        libc::MS_NOSUID | libc::MS_NODEV,
        Some(&options),
    )?;
    verify_workspace_root(checker_gid)
}

fn verify_workspace_root(checker_gid: u32) -> Result<(), String> {
    // SAFETY: fixed workspace path after the successful tmpfs mount.
    let directory = unsafe {
        libc::open(
            WORK_PATH.as_ptr(),
            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_DIRECTORY | libc::O_NOFOLLOW,
        )
    };
    if directory < 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    // SAFETY: open returned a fresh directory descriptor.
    let directory = unsafe { OwnedFd::from_raw_fd(directory) };
    let mut metadata: libc::stat = unsafe { mem::zeroed() };
    let mut filesystem: libc::statfs = unsafe { mem::zeroed() };
    if unsafe { libc::fstat(directory.as_raw_fd(), &mut metadata) } != 0
        || unsafe { libc::fstatfs(directory.as_raw_fd(), &mut filesystem) } != 0
    {
        return Err(io::Error::last_os_error().to_string());
    }
    const TMPFS_MAGIC: libc::c_long = 0x0102_1994;
    if metadata.st_uid != 0
        || metadata.st_gid != checker_gid
        || metadata.st_mode & libc::S_IFMT != libc::S_IFDIR
        || metadata.st_mode & 0o7777 != 0o2750
        || filesystem.f_type != TMPFS_MAGIC
    {
        return Err("workspace tmpfs metadata mismatch".to_string());
    }
    Ok(())
}

fn mount_raw(
    source: Option<&CStr>,
    target: &CStr,
    filesystem: Option<&CStr>,
    flags: libc::c_ulong,
    data: Option<&CStr>,
) -> Result<(), String> {
    // SAFETY: every optional pointer is either null or a live NUL-terminated
    // C string for the duration of mount(2).
    if unsafe {
        libc::mount(
            source.map_or(std::ptr::null(), |value| value.as_ptr()),
            target.as_ptr(),
            filesystem.map_or(std::ptr::null(), |value| value.as_ptr()),
            flags,
            data.map_or(std::ptr::null(), |value| value.as_ptr().cast()),
        )
    } != 0
    {
        return Err(io::Error::last_os_error().to_string());
    }
    Ok(())
}

fn materialize_authority(fd: RawFd, path: &CStr, checker_gid: u32) -> Result<(), String> {
    // SAFETY: fixed absolute path, exclusive create, no symlink following.
    let output = unsafe {
        libc::open(
            path.as_ptr(),
            libc::O_RDWR | libc::O_CREAT | libc::O_EXCL | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            0o440,
        )
    };
    if output < 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    // SAFETY: open returned a fresh descriptor.
    let mut output = unsafe { File::from_raw_fd(output) };
    let mut offset = 0_i64;
    let mut buffer = [0_u8; 8192];
    loop {
        // SAFETY: `buffer` is writable and `fd` is a sealed readable memfd.
        let read = unsafe { libc::pread(fd, buffer.as_mut_ptr().cast(), buffer.len(), offset) };
        if read < 0 {
            return Err(io::Error::last_os_error().to_string());
        }
        if read == 0 {
            break;
        }
        output
            .write_all(&buffer[..read as usize])
            .map_err(|error| error.to_string())?;
        offset += read as i64;
    }
    // The setgid tmpfs root supplies the checker group without CAP_CHOWN;
    // only the owner-controlled mode is tightened explicitly.
    if unsafe { libc::fchmod(output.as_raw_fd(), 0o440) } != 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    verify_materialized_authority(fd, &mut output, checker_gid)?;
    Ok(())
}

fn verify_materialized_authority(
    source_fd: RawFd,
    output: &mut File,
    checker_gid: u32,
) -> Result<(), String> {
    // SAFETY: both descriptors are live and both stat destinations are valid.
    let mut source_stat: libc::stat = unsafe { mem::zeroed() };
    let mut output_stat: libc::stat = unsafe { mem::zeroed() };
    if unsafe { libc::fstat(source_fd, &mut source_stat) } != 0
        || unsafe { libc::fstat(output.as_raw_fd(), &mut output_stat) } != 0
    {
        return Err(io::Error::last_os_error().to_string());
    }
    if source_stat.st_size != output_stat.st_size
        || output_stat.st_uid != 0
        || output_stat.st_gid != checker_gid
        || output_stat.st_mode & libc::S_IFMT != libc::S_IFREG
        || output_stat.st_mode & 0o7777 != 0o440
        || output_stat.st_nlink != 1
    {
        return Err("materialized authority metadata mismatch".to_string());
    }

    let mut offset = 0_i64;
    let mut source = [0_u8; 8192];
    let mut materialized = [0_u8; 8192];
    loop {
        // SAFETY: both buffers are writable and both descriptors are live.
        let source_read =
            unsafe { libc::pread(source_fd, source.as_mut_ptr().cast(), source.len(), offset) };
        let output_read = unsafe {
            libc::pread(
                output.as_raw_fd(),
                materialized.as_mut_ptr().cast(),
                materialized.len(),
                offset,
            )
        };
        if source_read < 0 || output_read < 0 {
            return Err(io::Error::last_os_error().to_string());
        }
        if source_read != output_read
            || source[..source_read as usize] != materialized[..output_read as usize]
        {
            return Err("materialized authority bytes mismatch".to_string());
        }
        if source_read == 0 {
            break;
        }
        offset += source_read as i64;
    }
    Ok(())
}

fn create_scratch(checker_gid: u32) -> Result<(), String> {
    // SAFETY: fixed path inside the fresh tmpfs.
    if unsafe { libc::mkdir(c"/work/scratch".as_ptr(), 0o2770) } != 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    if unsafe { libc::chmod(c"/work/scratch".as_ptr(), 0o2770) } != 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    // SAFETY: fixed nonsymlink directory just created inside the fresh tmpfs.
    let directory = unsafe {
        libc::open(
            c"/work/scratch".as_ptr(),
            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_DIRECTORY | libc::O_NOFOLLOW,
        )
    };
    if directory < 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    // SAFETY: open returned a fresh directory descriptor.
    let directory = unsafe { OwnedFd::from_raw_fd(directory) };
    let mut metadata: libc::stat = unsafe { mem::zeroed() };
    if unsafe { libc::fstat(directory.as_raw_fd(), &mut metadata) } != 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    if metadata.st_uid != 0
        || metadata.st_gid != checker_gid
        || metadata.st_mode & libc::S_IFMT != libc::S_IFDIR
        || metadata.st_mode & 0o7777 != 0o2770
    {
        return Err("scratch directory metadata mismatch".to_string());
    }
    Ok(())
}

fn set_working_directory() -> Result<(), String> {
    // SAFETY: WORK_PATH is the fixed, NUL-terminated workspace mountpoint.
    if unsafe { libc::chdir(WORK_PATH.as_ptr()) } != 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    Ok(())
}

fn install_stdio_and_close_fds(
    stdout_fd: RawFd,
    stderr_fd: RawFd,
    setup_fd: RawFd,
) -> Result<(), String> {
    // SAFETY: fixed /dev/null and exact descriptor reassignment.
    let null_fd = unsafe { libc::open(c"/dev/null".as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC) };
    if null_fd < 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    for (from, to) in [(null_fd, 0), (stdout_fd, 1), (stderr_fd, 2)] {
        if unsafe { libc::dup2(from, to) } < 0 {
            return Err(io::Error::last_os_error().to_string());
        }
    }
    if setup_fd != 3 {
        if unsafe { libc::dup3(setup_fd, 3, libc::O_CLOEXEC) } < 0 {
            return Err(io::Error::last_os_error().to_string());
        }
    } else if unsafe { libc::fcntl(3, libc::F_SETFD, libc::FD_CLOEXEC) } != 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    // SAFETY: fd 0..=3 are the complete fixed inherited set; every higher
    // authority/control/ledger descriptor must disappear before untrusted exec.
    if unsafe { libc::syscall(libc::SYS_close_range, 4_u32, u32::MAX, 0_u32) } != 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    Ok(())
}

fn drop_all_privileges(uid: u32, gid: u32) -> Result<(), String> {
    let cap_last = std::fs::read_to_string("/proc/sys/kernel/cap_last_cap")
        .map_err(|error| error.to_string())?
        .trim()
        .parse::<libc::c_int>()
        .map_err(|_| "cap_last_cap is malformed".to_string())?;
    for capability in 0..=cap_last {
        // SAFETY: the launcher still has CAP_SETPCAP; all bounding entries are
        // irreversibly removed before changing UID.
        if unsafe { libc::prctl(libc::PR_CAPBSET_DROP, capability, 0, 0, 0) } != 0 {
            return Err(io::Error::last_os_error().to_string());
        }
    }
    if unsafe { libc::setgroups(0, std::ptr::null()) } != 0
        || unsafe { libc::setresgid(gid, gid, gid) } != 0
        || unsafe { libc::setresuid(uid, uid, uid) } != 0
    {
        return Err(io::Error::last_os_error().to_string());
    }
    if unsafe {
        libc::prctl(
            libc::PR_CAP_AMBIENT,
            libc::PR_CAP_AMBIENT_CLEAR_ALL,
            0,
            0,
            0,
        )
    } != 0
        || unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) } != 0
    {
        return Err(io::Error::last_os_error().to_string());
    }
    Ok(())
}

fn verify_dropped_privileges(uid: u32, gid: u32) -> Result<(), String> {
    let mut real_uid = 0;
    let mut effective_uid = 0;
    let mut saved_uid = 0;
    let mut real_gid = 0;
    let mut effective_gid = 0;
    let mut saved_gid = 0;
    // SAFETY: all six destinations are valid writable integers.
    if unsafe { libc::getresuid(&mut real_uid, &mut effective_uid, &mut saved_uid) } != 0
        || unsafe { libc::getresgid(&mut real_gid, &mut effective_gid, &mut saved_gid) } != 0
    {
        return Err(io::Error::last_os_error().to_string());
    }
    if [real_uid, effective_uid, saved_uid] != [uid; 3]
        || [real_gid, effective_gid, saved_gid] != [gid; 3]
    {
        return Err("checker real/effective/saved identity mismatch".to_string());
    }
    // SAFETY: a zero-length query does not dereference the null pointer.
    if unsafe { libc::getgroups(0, std::ptr::null_mut()) } != 0 {
        return Err("checker retained supplementary groups".to_string());
    }

    let status = std::fs::read_to_string("/proc/self/status").map_err(|error| error.to_string())?;
    require_status_ids(&status, "Uid", uid)?;
    require_status_ids(&status, "Gid", gid)?;
    for field in ["CapInh", "CapPrm", "CapEff", "CapBnd", "CapAmb"] {
        let value = require_status_field(&status, field)?;
        if value.len() != 16
            || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
            || u64::from_str_radix(value, 16).map_err(|_| "malformed capability set")? != 0
        {
            return Err(format!("checker {field} is not exact empty"));
        }
    }
    if require_status_field(&status, "NoNewPrivs")? != "1" {
        return Err("checker no_new_privs was not enabled".to_string());
    }
    Ok(())
}

fn require_status_ids(status: &str, field: &str, expected: u32) -> Result<(), String> {
    let value = require_status_field(status, field)?;
    let ids = value
        .split_whitespace()
        .map(|part| part.parse::<u32>())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| format!("checker {field} is malformed"))?;
    if ids != [expected; 4] {
        return Err(format!("checker {field} identity mismatch"));
    }
    Ok(())
}

fn require_status_field<'a>(status: &'a str, field: &str) -> Result<&'a str, String> {
    let prefix = format!("{field}:");
    let mut values = status
        .lines()
        .filter_map(|line| line.strip_prefix(&prefix).map(str::trim));
    let value = values
        .next()
        .ok_or_else(|| format!("checker status lacks {field}"))?;
    if values.next().is_some() || value.is_empty() {
        return Err(format!("checker status has invalid {field} cardinality"));
    }
    Ok(value)
}

fn apply_outer_rlimits() -> Result<(), String> {
    for (resource, value) in [
        (libc::RLIMIT_CPU, 120_u64),
        (libc::RLIMIT_AS, 2_147_483_648_u64),
        (libc::RLIMIT_FSIZE, 67_108_864_u64),
        (libc::RLIMIT_NOFILE, 128_u64),
    ] {
        let limit = libc::rlimit {
            rlim_cur: value,
            rlim_max: value,
        };
        // SAFETY: fixed resource and limits, applied to the current child.
        if unsafe { libc::setrlimit(resource, &limit) } != 0 {
            return Err(io::Error::last_os_error().to_string());
        }
    }
    Ok(())
}

fn build_seccomp_program() -> Result<seccompiler::BpfProgram, ContainmentFailure> {
    let syscalls = [
        libc::SYS_accept,
        libc::SYS_accept4,
        libc::SYS_add_key,
        libc::SYS_bind,
        libc::SYS_bpf,
        libc::SYS_connect,
        libc::SYS_delete_module,
        libc::SYS_finit_module,
        libc::SYS_init_module,
        libc::SYS_io_uring_setup,
        libc::SYS_kexec_file_load,
        libc::SYS_kexec_load,
        libc::SYS_keyctl,
        libc::SYS_listen,
        libc::SYS_mount,
        libc::SYS_name_to_handle_at,
        libc::SYS_open_by_handle_at,
        libc::SYS_perf_event_open,
        libc::SYS_process_vm_readv,
        libc::SYS_process_vm_writev,
        libc::SYS_ptrace,
        libc::SYS_reboot,
        libc::SYS_recvfrom,
        libc::SYS_recvmsg,
        libc::SYS_request_key,
        libc::SYS_sendmsg,
        libc::SYS_sendto,
        libc::SYS_setns,
        libc::SYS_socket,
        libc::SYS_socketpair,
        libc::SYS_swapoff,
        libc::SYS_swapon,
        libc::SYS_umount2,
        libc::SYS_unshare,
        libc::SYS_userfaultfd,
    ];
    let rules = syscalls
        .into_iter()
        .map(|number| (number, vec![]))
        .collect();
    let arch = TargetArch::try_from(std::env::consts::ARCH).map_err(|_| {
        ContainmentFailure::Platform("unsupported seccomp target architecture".to_string())
    })?;
    let filter = SeccompFilter::new(
        rules,
        SeccompAction::Allow,
        SeccompAction::Errno(libc::EACCES as u32),
        arch,
    )
    .map_err(|error| ContainmentFailure::Platform(error.to_string()))?;
    seccompiler::BpfProgram::try_from(filter)
        .map_err(|error| ContainmentFailure::Platform(error.to_string()))
}

fn apply_landlock() -> Result<(), String> {
    let abi = ABI::V3;
    let write_access = AccessFs::from_write(abi);
    let handled = write_access | AccessFs::Execute;
    let mut ruleset = Ruleset::default()
        .set_compatibility(CompatLevel::HardRequirement)
        .handle_access(handled)
        .map_err(|error| error.to_string())?
        .create()
        .map_err(|error| error.to_string())?;
    for path in [
        "/bin",
        "/lib",
        "/lib64",
        "/usr/bin",
        "/usr/lib",
        "/opt/boole/native-checker-toolchain",
        "/work",
    ] {
        let fd = PathFd::new(path).map_err(|error| error.to_string())?;
        ruleset = ruleset
            .add_rule(PathBeneath::new(fd, AccessFs::Execute))
            .map_err(|error| error.to_string())?;
    }
    let work = PathFd::new("/work").map_err(|error| error.to_string())?;
    ruleset = ruleset
        .add_rule(PathBeneath::new(work, write_access))
        .map_err(|error| error.to_string())?;
    let status = ruleset.restrict_self().map_err(|error| error.to_string())?;
    if status.ruleset != RulesetStatus::FullyEnforced || !status.no_new_privs {
        return Err("Landlock was not fully enforced".to_string());
    }
    Ok(())
}

fn exec_checker() -> Result<(), String> {
    let argv = [
        CHECKER_PATH,
        c"-I",
        c"-S",
        CHECKER_SCRIPT,
        c"--task",
        c"/work/task.json",
        c"--submission",
        c"/work/submission.rs",
        c"--toolchain-bin",
        TOOLCHAIN_BIN,
        c"--scratch-root",
        c"/work/scratch",
    ];
    let mut argv_ptrs = argv.iter().map(|value| value.as_ptr()).collect::<Vec<_>>();
    argv_ptrs.push(std::ptr::null());
    let env = [
        c"PATH=/usr/bin:/bin:/usr/sbin:/sbin",
        c"LC_ALL=C",
        c"LANG=C",
        c"TZ=UTC",
        c"TERM=dumb",
        c"CARGO_NET_OFFLINE=true",
        c"HOME=/work/scratch",
        c"TMPDIR=/work/scratch",
    ];
    let mut env_ptrs = env.iter().map(|value| value.as_ptr()).collect::<Vec<_>>();
    env_ptrs.push(std::ptr::null());
    // SAFETY: all argv/env pointers remain live and NUL-terminated; success
    // replaces this process image and does not return.
    unsafe { libc::execve(CHECKER_PATH.as_ptr(), argv_ptrs.as_ptr(), env_ptrs.as_ptr()) };
    Err(io::Error::last_os_error().to_string())
}

fn write_setup_error(fd: RawFd, error: &str) {
    let bytes = error.as_bytes();
    let bounded = &bytes[..bytes.len().min(SETUP_ERROR_LIMIT)];
    // SAFETY: bounded points to live bytes and setup fd is still owned by the child.
    let _ = unsafe { libc::write(fd, bounded.as_ptr().cast(), bounded.len()) };
}

fn setup_error_fd(original: RawFd) -> RawFd {
    if original == 3 {
        return 3;
    }
    // Before the FD-block step the original pipe is live.  That step moves
    // the pipe to fixed FD 3 and closes every descriptor >= 4, so EBADF here
    // means all later setup errors must use FD 3 instead.
    // SAFETY: F_GETFD only observes the numeric descriptor.
    if unsafe { libc::fcntl(original, libc::F_GETFD) } >= 0 {
        original
    } else {
        3
    }
}

fn platform(error: impl std::fmt::Display) -> ContainmentFailure {
    ContainmentFailure::Platform(error.to_string())
}

fn io_platform(error: io::Error) -> ContainmentFailure {
    platform(error)
}

#[cfg(test)]
mod tests {
    use sha2::{Digest, Sha256};

    use super::{CapturedOutput, OutputStream, OUTPUT_LIMIT};

    #[test]
    fn stdout_and_stderr_share_one_hard_retention_ceiling() {
        let stdout = vec![b'a'; OUTPUT_LIMIT - 2];
        let stderr = b"wxyz";
        let mut captured = CapturedOutput::default();

        captured
            .observe(OutputStream::Stdout, &stdout)
            .expect("bounded stdout");
        captured
            .observe(OutputStream::Stderr, stderr)
            .expect("overflowing stderr");

        assert!(captured.overflow);
        assert_eq!(captured.retained_bytes, OUTPUT_LIMIT);
        let (retained_stdout, stdout_bytes, stdout_sha256) = captured.stdout.finish();
        let (retained_stderr, stderr_bytes, stderr_sha256) = captured.stderr.finish();
        assert_eq!(retained_stdout, stdout);
        assert_eq!(retained_stderr, b"wx");
        assert_eq!(stdout_bytes, (OUTPUT_LIMIT - 2) as u64);
        assert_eq!(stderr_bytes, 4);
        assert_eq!(stdout_sha256, <[u8; 32]>::from(Sha256::digest(&stdout)));
        assert_eq!(stderr_sha256, <[u8; 32]>::from(Sha256::digest(stderr)));
    }
}
