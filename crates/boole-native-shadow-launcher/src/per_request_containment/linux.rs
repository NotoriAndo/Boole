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
use std::path::Path;
use std::time::{Duration, Instant};

use landlock::{
    AccessFs, CompatLevel, Compatible, PathBeneath, PathFd, Ruleset, RulesetAttr,
    RulesetCreatedAttr, RulesetStatus, ABI,
};
use seccompiler::{
    SeccompAction, SeccompCmpArgLen, SeccompCmpOp, SeccompCondition, SeccompFilter, SeccompRule,
    TargetArch,
};
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
const FIXED_LAUNCHER_UMASK: libc::mode_t = 0o117;
const CHECKER_UMASK: libc::mode_t = 0o077;

const CHECKER_PATH: &CStr = c"/usr/bin/python3.12";
const CHECKER_SCRIPT: &CStr =
    c"/usr/share/boole/native-shadow/checkers/rust-tuple-struct-project-v1/checker.py";
const TOOLCHAIN_BIN: &CStr = c"/opt/boole/native-checker-toolchain/bin";
const WORK_PATH: &CStr = c"/work";
const PROC_PATH: &CStr = c"/proc";
const TMP_PATH: &CStr = c"/tmp";
const STAGING_PATH: &CStr = c"/run";
const VERIFIED_RUNTIME_ROOTFS_PATH: &CStr = c"/var/lib/boole/native-shadow/runtime-rootfs";
const RUNTIME_BASE: &CStr = c"/run/boole/native-shadow";
const RUNTIME_LOWER: &CStr = c"/run/boole/native-shadow/rootfs-lower";
const RUNTIME_ADDITIONS: &CStr = c"/run/boole/native-shadow/rootfs-additions";
const RUNTIME_ADDITIONS_ETC: &CStr = c"/run/boole/native-shadow/rootfs-additions/etc";
const RUNTIME_ADDITIONS_PASSWD: &CStr = c"/run/boole/native-shadow/rootfs-additions/etc/passwd";
const RUNTIME_ROOT: &CStr = c"/run/boole/native-shadow/rootfs-root";
const RUNTIME_ROOT_PASSWD: &CStr = c"/run/boole/native-shadow/rootfs-root/etc/passwd";
const RUNTIME_ROOT_DEV: &CStr = c"/run/boole/native-shadow/rootfs-root/dev";
const RUNTIME_ROOT_DEV_NULL: &CStr = c"/run/boole/native-shadow/rootfs-root/dev/null";

pub(super) fn execute(
    compatibility: &VerifiedStartupToolchainCompatibility,
    materials: VerifiedCheckerMaterials,
) -> Result<ContainedExecution, ContainmentFailure> {
    let recovery = compatibility.recovery();
    recovery
        .verify_cgroup_state_after_trusted_probes()
        .map_err(ContainmentFailure::Platform)?;
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
        diagnostic_layers: selected_diagnostic_layers(),
    };
    execute_with_operations(operations.materials.operation, &mut operations)
}

struct LinuxOperations<'a> {
    service_root: &'a cgroupfs_fd::CgroupDirectory,
    checker_uid: u32,
    checker_gid: u32,
    materials: VerifiedCheckerMaterials,
    diagnostic_layers: DiagnosticLayers,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct DiagnosticLayers {
    skip_cgroup_limits: bool,
    skip_rlimits: bool,
    skip_landlock: bool,
    skip_seccomp: bool,
}

fn selected_diagnostic_layers() -> DiagnosticLayers {
    #[cfg(feature = "manager-cgroup-linux-gate")]
    {
        let mode = super::containment_diagnostic_mode();
        DiagnosticLayers {
            skip_cgroup_limits: mode.skips_cgroup_limits(),
            skip_rlimits: mode.skips_rlimits(),
            skip_landlock: mode.skips_landlock(),
            skip_seccomp: mode.skips_seccomp(),
        }
    }
    #[cfg(not(feature = "manager-cgroup-linux-gate"))]
    {
        DiagnosticLayers::default()
    }
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
struct RuntimeRootIdentity {
    device: u64,
    inode: u64,
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
        if self.diagnostic_layers.skip_cgroup_limits {
            return Ok(());
        }
        cgroupfs_fd::apply_execution_leaf_limits(leaf).map_err(platform)
    }

    fn clone_child_atomically(
        &mut self,
        leaf: &Self::Leaf,
    ) -> Result<Self::Child, ContainmentFailure> {
        clone_contained_child(
            leaf,
            self.checker_uid,
            self.checker_gid,
            &self.materials,
            self.diagnostic_layers,
        )
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
    diagnostic_layers: DiagnosticLayers,
) -> Result<LinuxChild, ContainmentFailure> {
    let host_dev_null = open_verified_host_dev_null()?;
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

    // The wall deadline starts before clone3 so kernel scheduling and setup
    // time cannot be omitted from the fixed outer budget.
    let started = Instant::now();
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
            rootfs_fd: materials.rootfs.as_raw_fd(),
            dev_null_fd: host_dev_null.as_raw_fd(),
            task_fd: task.as_raw_fd(),
            anchor_fd: anchor.as_raw_fd(),
            submission_fd: submission.as_raw_fd(),
            stdout_fd: stdout_write.as_raw_fd(),
            stderr_fd: stderr_write.as_raw_fd(),
            setup_fd,
            seccomp,
            diagnostic_layers,
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
        host_dev_null,
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
        started,
        output: None,
        reaped: false,
    })
}

struct ChildSetup {
    checker_uid: u32,
    checker_gid: u32,
    rootfs_fd: RawFd,
    dev_null_fd: RawFd,
    task_fd: RawFd,
    anchor_fd: RawFd,
    submission_fd: RawFd,
    stdout_fd: RawFd,
    stderr_fd: RawFd,
    setup_fd: RawFd,
    seccomp: seccompiler::BpfProgram,
    diagnostic_layers: DiagnosticLayers,
}

fn child_setup_and_exec(setup: ChildSetup) -> Result<(), String> {
    setup_stage(
        "derive-runtime-root",
        derive_and_enter_runtime_root(
            setup.rootfs_fd,
            setup.dev_null_fd,
            setup.checker_uid,
            setup.checker_gid,
        ),
    )?;
    setup_stage(
        "mount-private-filesystems",
        mount_private_filesystems(setup.checker_gid, setup.dev_null_fd),
    )?;
    setup_stage(
        "materialize-task",
        materialize_authority(setup.task_fd, c"/work/task.json", setup.checker_gid),
    )?;
    setup_stage(
        "materialize-anchor",
        materialize_authority(setup.anchor_fd, c"/work/anchor.rs", setup.checker_gid),
    )?;
    setup_stage(
        "materialize-submission",
        materialize_authority(
            setup.submission_fd,
            c"/work/submission.rs",
            setup.checker_gid,
        ),
    )?;
    setup_stage("create-scratch", create_scratch(setup.checker_gid))?;
    setup_stage("set-working-directory", set_working_directory())?;
    setup_stage(
        "install-stdio",
        install_stdio_and_close_fds(setup.stdout_fd, setup.stderr_fd, setup.setup_fd),
    )?;
    setup_stage(
        "drop-privileges",
        drop_all_privileges(setup.checker_uid, setup.checker_gid),
    )?;
    setup_stage(
        "verify-privileges",
        verify_dropped_privileges(setup.checker_uid, setup.checker_gid),
    )?;
    setup_stage(
        "verify-runtime-identity-lookup",
        verify_runtime_identity_lookup(setup.checker_uid, setup.checker_gid),
    )?;
    setup_stage("set-checker-umask", set_checker_umask())?;
    if !setup.diagnostic_layers.skip_rlimits {
        setup_stage("apply-rlimits", apply_outer_rlimits())?;
    }
    if !setup.diagnostic_layers.skip_landlock {
        setup_stage("install-landlock", apply_landlock())?;
    }
    if !setup.diagnostic_layers.skip_seccomp {
        setup_stage(
            "install-seccomp",
            seccompiler::apply_filter(&setup.seccomp).map_err(|error| error.to_string()),
        )?;
    }
    setup_stage("exec-checker", exec_checker())
}

fn setup_stage<T>(stage: &'static str, result: Result<T, String>) -> Result<T, String> {
    result.map_err(|error| format!("{stage}: {error}"))
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
    drain_after_exit_to_eof(&mut child.stdout, &mut output, OutputStream::Stdout)?;
    drain_after_exit_to_eof(&mut child.stderr, &mut output, OutputStream::Stderr)?;
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

fn drain_after_exit_to_eof(
    file: &mut File,
    output: &mut CapturedOutput,
    stream: OutputStream,
) -> Result<(), ContainmentFailure> {
    set_nonblocking(file.as_raw_fd(), false)?;
    let mut buffer = [0_u8; 8192];
    loop {
        match file.read(&mut buffer) {
            Ok(0) => return Ok(()),
            Ok(read) => output.observe(stream, &buffer[..read])?,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(io_platform(error)),
        }
    }
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

fn derive_and_enter_runtime_root(
    rootfs_fd: RawFd,
    dev_null_fd: RawFd,
    setup_checker_uid: u32,
    setup_checker_gid: u32,
) -> Result<(), String> {
    setup_stage(
        "derive-make-root-private",
        mount_raw(None, c"/", None, libc::MS_REC | libc::MS_PRIVATE, None),
    )?;
    setup_stage(
        "derive-check-lower",
        require_runtime_paths_absent_from_frozen_lower(rootfs_fd),
    )?;

    let staging_options = c"size=67108864,nr_inodes=4096,mode=0700,uid=0,gid=0";
    setup_stage(
        "derive-mount-staging",
        mount_raw(
            Some(c"tmpfs"),
            STAGING_PATH,
            Some(c"tmpfs"),
            // This tmpfs supplies the fixed, read-only additions layer for
            // the merged executable root. Marking the backing mount noexec
            // can make the kernel reject execution through the merged mount.
            // The staging tree remains root-only and disappears with the
            // private mount namespace; writable untrusted paths receive their
            // own reviewed mount flags later.
            libc::MS_NOSUID | libc::MS_NODEV,
            Some(staging_options),
        ),
    )?;
    setup_stage("derive-create-staging", create_runtime_staging_tree())?;
    setup_stage(
        "derive-materialize-passwd",
        materialize_runtime_passwd(setup_checker_uid, setup_checker_gid),
    )?;

    // The rootfs has one fixed systemd BindReadOnlyPaths location. Verify that
    // location against the retained descriptor immediately before the bind,
    // then verify the resulting lower mount against the descriptor again.
    // The post-bind check makes a source-path race fail closed before any
    // untrusted bytes are materialized or executed. This avoids open_tree(2),
    // which is rejected by the supported GitHub runner's nested mount setup.
    setup_stage(
        "derive-verify-fixed-lower-path",
        verify_fixed_runtime_root_path(rootfs_fd),
    )?;
    setup_stage(
        "derive-bind-lower",
        mount_raw(
            Some(VERIFIED_RUNTIME_ROOTFS_PATH),
            RUNTIME_LOWER,
            None,
            libc::MS_BIND,
            None,
        ),
    )?;
    setup_stage(
        "derive-remount-lower-read-only",
        mount_raw(
            None,
            RUNTIME_LOWER,
            None,
            libc::MS_BIND | libc::MS_REMOUNT | libc::MS_RDONLY | libc::MS_NOSUID | libc::MS_NODEV,
            None,
        ),
    )?;
    setup_stage("derive-verify-bound-lower", verify_bound_lower(rootfs_fd))?;

    let overlay_options = CString::new(format!(
        "lowerdir={}:{}",
        RUNTIME_ADDITIONS.to_string_lossy(),
        RUNTIME_LOWER.to_string_lossy(),
    ))
    .map_err(|_| "overlay mount options contain NUL".to_string())?;
    setup_stage("derive-mount-overlay", {
        let result = mount_raw(
            Some(c"overlay"),
            RUNTIME_ROOT,
            Some(c"overlay"),
            libc::MS_RDONLY | libc::MS_NOSUID | libc::MS_NODEV,
            Some(&overlay_options),
        );
        result.map_err(|error| overlay_mount_failure_context(&error))
    })?;
    let derived_root_identity = setup_stage(
        "derive-verify-overlay",
        verify_derived_runtime_root(rootfs_fd, setup_checker_uid, setup_checker_gid),
    )?;
    // The inherited host device FD predates CLONE_NEWNS, so it is identity
    // authority only. The bind helper reopens that exact device inside the
    // child mount namespace before placing it below RUNTIME_ROOT; the private
    // /dev then crosses the pivot with the verified overlay.
    setup_stage("mount-private-dev", mount_private_dev(RUNTIME_ROOT_DEV))?;
    setup_stage(
        "bind-private-dev-null",
        bind_and_verify_dev_null(dev_null_fd, RUNTIME_ROOT_DEV_NULL),
    )?;
    // CAP_SYS_CHROOT is deliberately absent.  The child already owns a
    // private mount namespace and CAP_SYS_ADMIN, so atomically replace its
    // mount root instead of widening the capability set.
    // SAFETY: RUNTIME_ROOT is the verified private overlay mount.
    setup_stage(
        "derive-enter-overlay",
        syscall_zero(unsafe { libc::chdir(RUNTIME_ROOT.as_ptr()) } as libc::c_long),
    )?;
    // SAFETY: the documented same-directory pivot stacks the previous root
    // over the new root without requiring a writable put_old directory.  The
    // verified overlay itself is read-only.
    setup_stage(
        "derive-pivot-root",
        syscall_zero(unsafe { libc::syscall(libc::SYS_pivot_root, c".".as_ptr(), c".".as_ptr()) }),
    )?;
    // SAFETY: after the same-directory pivot, `.` names the old root mount
    // stacked over the retained new-root cwd. Detach it before changing cwd or
    // materializing any untrusted checker bytes.
    setup_stage(
        "derive-detach-old-root",
        syscall_zero(unsafe { libc::umount2(c".".as_ptr(), libc::MNT_DETACH) } as libc::c_long),
    )?;
    // SAFETY: the old root is detached, so `/` resolves inside the verified
    // read-only overlay.
    setup_stage(
        "derive-enter-new-root",
        syscall_zero(unsafe { libc::chdir(c"/".as_ptr()) } as libc::c_long),
    )?;
    setup_stage(
        "derive-verify-entered-root",
        verify_entered_runtime_root(derived_root_identity),
    )?;
    Ok(())
}

fn create_runtime_staging_tree() -> Result<(), String> {
    for path in [c"/run/boole", RUNTIME_BASE, RUNTIME_LOWER, RUNTIME_ROOT] {
        mkdir_fixed(path, 0o700)?;
    }
    // The first read-only lower layer supplies the merged root metadata, one
    // reviewed `/etc/passwd` shadow, and exactly four otherwise-absent
    // mountpoints. The untrusted checker runs after dropping root, so the
    // merged `/` must remain traversable while every staging parent stays
    // root-only.
    mkdir_fixed(RUNTIME_ADDITIONS, 0o755)?;
    for (path, mode) in [
        (RUNTIME_ADDITIONS_ETC, 0o755),
        (
            c"/run/boole/native-shadow/rootfs-additions/work" as &CStr,
            0o755,
        ),
        (
            c"/run/boole/native-shadow/rootfs-additions/proc" as &CStr,
            0o555,
        ),
        (
            c"/run/boole/native-shadow/rootfs-additions/dev" as &CStr,
            0o755,
        ),
        (
            c"/run/boole/native-shadow/rootfs-additions/tmp" as &CStr,
            0o1777,
        ),
    ] {
        mkdir_fixed(path, mode)?;
    }
    Ok(())
}

fn runtime_passwd_record(checker_uid: u32, checker_gid: u32) -> Result<Vec<u8>, String> {
    if checker_uid == 0 || checker_gid == 0 {
        return Err("runtime checker identity must be non-root".to_string());
    }
    Ok(format!(
        "boole-native-checker:x:{checker_uid}:{checker_gid}:boole-native-checker:/work/scratch:/usr/sbin/nologin\n"
    )
    .into_bytes())
}

fn materialize_runtime_passwd(checker_uid: u32, checker_gid: u32) -> Result<(), String> {
    let expected = runtime_passwd_record(checker_uid, checker_gid)?;
    // SAFETY: fixed root-only staging path, exclusive create and no symlink
    // following. The resulting file is exposed only through the read-only
    // overlay and never mutates the verified lower rootfs.
    let output = unsafe {
        libc::open(
            RUNTIME_ADDITIONS_PASSWD.as_ptr(),
            libc::O_RDWR | libc::O_CREAT | libc::O_EXCL | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            0o444,
        )
    };
    if output < 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    // SAFETY: open returned one fresh descriptor owned by this child.
    let mut output = unsafe { File::from_raw_fd(output) };
    output
        .write_all(&expected)
        .map_err(|error| error.to_string())?;
    // The service umask is intentionally restrictive, so fix the reviewed
    // read-only mode explicitly before any overlay mount can expose the file.
    if unsafe { libc::fchmod(output.as_raw_fd(), 0o444) } != 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    verify_runtime_passwd(&mut output, &expected)
}

fn verify_runtime_passwd(output: &mut File, expected: &[u8]) -> Result<(), String> {
    let mut metadata: libc::stat = unsafe { mem::zeroed() };
    // SAFETY: output is a live descriptor and metadata is writable.
    if unsafe { libc::fstat(output.as_raw_fd(), &mut metadata) } != 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    if metadata.st_uid != 0
        || metadata.st_gid != 0
        || metadata.st_mode & libc::S_IFMT != libc::S_IFREG
        || metadata.st_mode & 0o7777 != 0o444
        || metadata.st_nlink != 1
        || metadata.st_size != expected.len() as i64
    {
        return Err("runtime passwd metadata mismatch".to_string());
    }
    output
        .seek(SeekFrom::Start(0))
        .map_err(|error| error.to_string())?;
    let mut observed = Vec::with_capacity(expected.len());
    output
        .read_to_end(&mut observed)
        .map_err(|error| error.to_string())?;
    if observed != expected {
        return Err("runtime passwd bytes mismatch".to_string());
    }
    Ok(())
}

fn verify_derived_runtime_passwd(checker_uid: u32, checker_gid: u32) -> Result<(), String> {
    let expected = runtime_passwd_record(checker_uid, checker_gid)?;
    // SAFETY: fixed path in the just-mounted read-only overlay; no symlink is
    // accepted in place of the reviewed regular file.
    let output = unsafe {
        libc::open(
            RUNTIME_ROOT_PASSWD.as_ptr(),
            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if output < 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    // SAFETY: open returned one fresh descriptor.
    let mut output = unsafe { File::from_raw_fd(output) };
    verify_runtime_passwd(&mut output, &expected)
}

fn verify_fixed_runtime_root_path(rootfs_fd: RawFd) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;

    let mut expected: libc::stat = unsafe { mem::zeroed() };
    // SAFETY: rootfs_fd is the already-verified, live rootfs directory.
    if unsafe { libc::fstat(rootfs_fd, &mut expected) } != 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    let observed =
        std::fs::symlink_metadata(VERIFIED_RUNTIME_ROOTFS_PATH.to_string_lossy().as_ref())
            .map_err(|error| error.to_string())?;
    if !observed.file_type().is_dir()
        || observed.dev() != expected.st_dev
        || observed.ino() != expected.st_ino
    {
        return Err("fixed runtime rootfs path identity mismatch".to_string());
    }
    let mut filesystem: libc::statvfs = unsafe { mem::zeroed() };
    // SAFETY: the fixed path and writable statvfs storage remain live.
    if unsafe { libc::statvfs(VERIFIED_RUNTIME_ROOTFS_PATH.as_ptr(), &mut filesystem) } != 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    if filesystem.f_flag & libc::ST_RDONLY == 0 {
        return Err("fixed runtime rootfs path is not read-only".to_string());
    }
    Ok(())
}

fn verify_bound_lower(rootfs_fd: RawFd) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;

    let mut expected: libc::stat = unsafe { mem::zeroed() };
    // SAFETY: rootfs_fd is the already-verified, live rootfs directory.
    if unsafe { libc::fstat(rootfs_fd, &mut expected) } != 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    let observed = std::fs::symlink_metadata(RUNTIME_LOWER.to_string_lossy().as_ref())
        .map_err(|error| error.to_string())?;
    if !observed.file_type().is_dir()
        || observed.dev() != expected.st_dev
        || observed.ino() != expected.st_ino
    {
        return Err("bound lower root identity mismatch".to_string());
    }

    let mut filesystem: libc::statvfs = unsafe { mem::zeroed() };
    // SAFETY: fixed live bind mount path and writable statvfs storage.
    if unsafe { libc::statvfs(RUNTIME_LOWER.as_ptr(), &mut filesystem) } != 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    if filesystem.f_flag & libc::ST_RDONLY == 0 {
        return Err("bound lower root is not read-only".to_string());
    }
    Ok(())
}

fn syscall_zero(status: libc::c_long) -> Result<(), String> {
    if status == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error().to_string())
    }
}

fn overlay_mount_failure_context(error: &str) -> String {
    let capabilities = std::fs::read_to_string("/proc/self/status")
        .ok()
        .map(|status| {
            status
                .lines()
                .filter(|line| {
                    line.starts_with("Uid:")
                        || line.starts_with("Gid:")
                        || line.starts_with("CapPrm:")
                        || line.starts_with("CapEff:")
                        || line.starts_with("CapBnd:")
                })
                .collect::<Vec<_>>()
                .join(",")
        })
        .unwrap_or_else(|| "unavailable".to_string());
    let security_profile = std::fs::read_to_string("/proc/self/attr/current")
        .map(|value| value.trim().to_string())
        .unwrap_or_else(|_| "unavailable".to_string());
    let paths = [
        ("lower", RUNTIME_LOWER),
        ("additions", RUNTIME_ADDITIONS),
        ("target", RUNTIME_ROOT),
    ]
    .into_iter()
    .map(|(label, path)| overlay_path_diagnostic(label, path))
    .collect::<Vec<_>>()
    .join(";");
    format!("{error}; overlay-context profile={security_profile};{capabilities};{paths}")
}

fn overlay_path_diagnostic(label: &str, path: &CStr) -> String {
    let mut metadata: libc::stat = unsafe { mem::zeroed() };
    let mut filesystem: libc::statvfs = unsafe { mem::zeroed() };
    // SAFETY: each caller supplies a fixed live C path and writable structs.
    let stat_status = unsafe { libc::lstat(path.as_ptr(), &mut metadata) };
    // SAFETY: same fixed path and live output storage.
    let statvfs_status = unsafe { libc::statvfs(path.as_ptr(), &mut filesystem) };
    if stat_status != 0 || statvfs_status != 0 {
        return format!("{label}=unavailable({})", io::Error::last_os_error());
    }
    // SAFETY: fixed path and a read/write/search access probe with no mutation.
    let access = unsafe { libc::access(path.as_ptr(), libc::R_OK | libc::W_OK | libc::X_OK) };
    format!(
        "{label}=mode:{:o},uid:{},gid:{},dev:{},ino:{},vfs_flags:{:#x},rwx:{}",
        metadata.st_mode & 0o7777,
        metadata.st_uid,
        metadata.st_gid,
        metadata.st_dev,
        metadata.st_ino,
        filesystem.f_flag,
        access == 0
    )
}

fn require_runtime_paths_absent_from_frozen_lower(rootfs_fd: RawFd) -> Result<(), String> {
    for name in [c"work", c"proc", c"dev", c"tmp"] {
        let mut metadata: libc::stat = unsafe { mem::zeroed() };
        // SAFETY: rootfs_fd is the verified directory and names are fixed,
        // single-component, NUL-terminated paths.
        let result = unsafe {
            libc::fstatat(
                rootfs_fd,
                name.as_ptr(),
                &mut metadata,
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        if result == 0 {
            return Err(format!(
                "frozen rootfs unexpectedly preinstalls /{}",
                name.to_string_lossy()
            ));
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::ENOENT) {
            return Err(error.to_string());
        }
    }
    Ok(())
}

fn verify_derived_runtime_root(
    rootfs_fd: RawFd,
    checker_uid: u32,
    checker_gid: u32,
) -> Result<RuntimeRootIdentity, String> {
    use std::collections::BTreeSet;
    use std::os::unix::fs::MetadataExt;

    let root_metadata = std::fs::symlink_metadata(RUNTIME_ROOT.to_string_lossy().as_ref())
        .map_err(|error| error.to_string())?;
    if !runtime_root_metadata_is_exact(
        root_metadata.uid(),
        root_metadata.gid(),
        root_metadata.mode(),
    ) {
        return Err("derived runtime root metadata mismatch".to_string());
    }

    let lower = Path::new(&format!("/proc/self/fd/{rootfs_fd}"))
        .read_dir()
        .map_err(|error| error.to_string())?
        .map(|entry| entry.map(|value| value.file_name()))
        .collect::<Result<BTreeSet<_>, _>>()
        .map_err(|error| error.to_string())?;
    let observed = Path::new("/run/boole/native-shadow/rootfs-root")
        .read_dir()
        .map_err(|error| error.to_string())?
        .map(|entry| entry.map(|value| value.file_name()))
        .collect::<Result<BTreeSet<_>, _>>()
        .map_err(|error| error.to_string())?;
    if !derived_runtime_top_level_is_exact(&lower, &observed) {
        return Err("derived runtime root top-level allowlist mismatch".to_string());
    }
    for (name, mode) in [
        ("etc", 0o755),
        ("work", 0o755),
        ("proc", 0o555),
        ("dev", 0o755),
        ("tmp", 0o1777),
    ] {
        let metadata =
            std::fs::symlink_metadata(Path::new("/run/boole/native-shadow/rootfs-root").join(name))
                .map_err(|error| error.to_string())?;
        if !metadata.file_type().is_dir()
            || metadata.uid() != 0
            || metadata.gid() != 0
            || metadata.mode() & 0o7777 != mode
        {
            return Err(format!("derived runtime root /{name} metadata mismatch"));
        }
    }
    verify_derived_runtime_passwd(checker_uid, checker_gid)?;
    let mut filesystem: libc::statfs = unsafe { mem::zeroed() };
    // SAFETY: fixed verified overlay root path and writable statfs storage.
    if unsafe { libc::statfs(RUNTIME_ROOT.as_ptr(), &mut filesystem) } != 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    const OVERLAYFS_MAGIC: libc::c_long = 0x794c_7630;
    if filesystem.f_type != OVERLAYFS_MAGIC {
        return Err("derived runtime root is not overlayfs".to_string());
    }
    let mut mount: libc::statvfs = unsafe { mem::zeroed() };
    // SAFETY: fixed verified overlay root path and writable statvfs storage.
    if unsafe { libc::statvfs(RUNTIME_ROOT.as_ptr(), &mut mount) } != 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    if mount.f_flag & libc::ST_RDONLY == 0 {
        return Err("derived runtime root is not read-only".to_string());
    }
    Ok(RuntimeRootIdentity {
        device: root_metadata.dev(),
        inode: root_metadata.ino(),
    })
}

fn verify_entered_runtime_root(expected: RuntimeRootIdentity) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;

    let root = std::fs::symlink_metadata("/").map_err(|error| error.to_string())?;
    if !runtime_root_metadata_is_exact(root.uid(), root.gid(), root.mode()) {
        return Err("entered runtime root metadata mismatch".to_string());
    }
    if root.dev() != expected.device || root.ino() != expected.inode {
        return Err("entered runtime root identity mismatch".to_string());
    }
    let mut filesystem: libc::statfs = unsafe { mem::zeroed() };
    // SAFETY: fixed entered root path and writable statfs storage.
    if unsafe { libc::statfs(c"/".as_ptr(), &mut filesystem) } != 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    const OVERLAYFS_MAGIC: libc::c_long = 0x794c_7630;
    if filesystem.f_type != OVERLAYFS_MAGIC {
        return Err("entered runtime root is not overlayfs".to_string());
    }
    let mut mount: libc::statvfs = unsafe { mem::zeroed() };
    // SAFETY: fixed entered root path and writable statvfs storage.
    if unsafe { libc::statvfs(c"/".as_ptr(), &mut mount) } != 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    if mount.f_flag & libc::ST_RDONLY == 0 {
        return Err("entered runtime root is not read-only".to_string());
    }
    Ok(())
}

fn runtime_root_metadata_is_exact(uid: u32, gid: u32, mode: u32) -> bool {
    uid == 0 && gid == 0 && mode & libc::S_IFMT == libc::S_IFDIR && mode & 0o7777 == 0o755
}

fn runtime_additions_are_compatible_with_lower(
    lower: &std::collections::BTreeSet<std::ffi::OsString>,
) -> bool {
    let reviewed_overlap = std::ffi::OsString::from("etc");
    let exclusive_additions = ["work", "proc", "dev", "tmp"]
        .into_iter()
        .map(std::ffi::OsString::from)
        .collect::<std::collections::BTreeSet<_>>();
    lower.contains(&reviewed_overlap) && lower.is_disjoint(&exclusive_additions)
}

fn derived_runtime_top_level_is_exact(
    lower: &std::collections::BTreeSet<std::ffi::OsString>,
    observed: &std::collections::BTreeSet<std::ffi::OsString>,
) -> bool {
    if !runtime_additions_are_compatible_with_lower(lower) {
        return false;
    }
    let additions = ["etc", "work", "proc", "dev", "tmp"]
        .into_iter()
        .map(std::ffi::OsString::from)
        .collect::<std::collections::BTreeSet<_>>();
    let expected = lower.union(&additions).cloned().collect();
    observed == &expected
}

fn mkdir_fixed(path: &CStr, mode: libc::mode_t) -> Result<(), String> {
    // SAFETY: every path is fixed and lies below the child-private tmpfs.
    if unsafe { libc::mkdir(path.as_ptr(), mode) } != 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    // mkdir is affected by umask; enforce the exact reviewed mode.
    if unsafe { libc::chmod(path.as_ptr(), mode) } != 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    Ok(())
}

fn mount_private_filesystems(checker_gid: u32, dev_null_fd: RawFd) -> Result<(), String> {
    setup_stage(
        "mount-private-proc",
        mount_raw(
            Some(c"proc"),
            PROC_PATH,
            Some(c"proc"),
            libc::MS_NOSUID | libc::MS_NODEV | libc::MS_NOEXEC,
            None,
        ),
    )?;
    let options = CString::new(format!(
        "size=536870912,nr_inodes=8192,mode=2750,uid=0,gid={checker_gid}"
    ))
    .map_err(|_| "tmpfs mount options contain NUL".to_string())?;
    setup_stage(
        "mount-private-work",
        mount_raw(
            Some(c"tmpfs"),
            WORK_PATH,
            Some(c"tmpfs"),
            libc::MS_NOSUID | libc::MS_NODEV,
            Some(&options),
        ),
    )?;
    let tmp_options = c"size=67108864,nr_inodes=2048,mode=1777,uid=0,gid=0";
    setup_stage(
        "mount-private-tmp",
        mount_raw(
            Some(c"tmpfs"),
            TMP_PATH,
            Some(c"tmpfs"),
            libc::MS_NOSUID | libc::MS_NODEV | libc::MS_NOEXEC,
            Some(tmp_options),
        ),
    )?;
    setup_stage(
        "verify-private-dev-null",
        verify_bound_dev_null(dev_null_fd, c"/dev/null"),
    )?;
    setup_stage("verify-private-work", verify_workspace_root(checker_gid))
}

fn mount_private_dev(target: &CStr) -> Result<(), String> {
    let dev_options = c"size=1048576,nr_inodes=16,mode=0755,uid=0,gid=0";
    mount_raw(
        Some(c"tmpfs"),
        target,
        Some(c"tmpfs"),
        libc::MS_NOSUID | libc::MS_NOEXEC,
        Some(dev_options),
    )
}

fn bind_and_verify_dev_null(source_fd: RawFd, target: &CStr) -> Result<(), String> {
    // `source_fd` was opened by the parent before CLONE_NEWNS. Its file
    // identity is still authoritative, but its `/proc/self/fd/N` magic link
    // points at the parent's mount object and Linux rejects that object as an
    // MS_BIND source in the child's private namespace. Reopen the exact
    // device after CLONE_NEWNS and bind only that child-local descriptor.
    let child_local = open_verified_child_dev_null(source_fd)?;
    let child_local_fd = child_local.as_raw_fd();
    // SAFETY: /dev is a fresh child-private tmpfs and this creates only the
    // fixed bind target. No device node is synthesized and CAP_MKNOD is not
    // required or granted.
    let placeholder = unsafe {
        libc::open(
            target.as_ptr(),
            libc::O_RDONLY | libc::O_CREAT | libc::O_EXCL | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            0o600,
        )
    };
    if placeholder < 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    // SAFETY: open returned one fresh descriptor.
    drop(unsafe { OwnedFd::from_raw_fd(placeholder) });
    let source = CString::new(format!("/proc/self/fd/{child_local_fd}"))
        .map_err(|_| "fixed child-local /dev/null source contains NUL".to_string())?;
    mount_raw(Some(&source), target, None, libc::MS_BIND, None)?;
    verify_bound_dev_null(source_fd, target)
}

fn open_verified_child_dev_null(parent_fd: RawFd) -> Result<OwnedFd, String> {
    // SAFETY: exact fixed path in the child's private mount namespace, with
    // symlink traversal disabled.
    let fd = unsafe {
        libc::open(
            c"/dev/null".as_ptr(),
            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    // SAFETY: open returned one fresh descriptor.
    let child_local = unsafe { OwnedFd::from_raw_fd(fd) };
    let parent_stat = fd_stat(parent_fd)?;
    let child_stat = fd_stat(child_local.as_raw_fd())?;
    if !is_exact_dev_null(&parent_stat)
        || !is_exact_dev_null(&child_stat)
        || parent_stat.st_dev != child_stat.st_dev
        || parent_stat.st_ino != child_stat.st_ino
    {
        return Err("child-local /dev/null identity mismatch".to_string());
    }
    Ok(child_local)
}

fn verify_bound_dev_null(source_fd: RawFd, target: &CStr) -> Result<(), String> {
    let source_stat = fd_stat(source_fd)?;
    let mut target_stat: libc::stat = unsafe { mem::zeroed() };
    // SAFETY: caller supplies one of the two fixed nonsymlink bind targets and
    // target_stat is writable metadata storage.
    if unsafe { libc::lstat(target.as_ptr(), &mut target_stat) } != 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    if !is_exact_dev_null(&source_stat)
        || !is_exact_dev_null(&target_stat)
        || source_stat.st_dev != target_stat.st_dev
        || source_stat.st_ino != target_stat.st_ino
    {
        return Err("derived runtime /dev/null identity mismatch".to_string());
    }
    Ok(())
}

fn open_verified_host_dev_null() -> Result<OwnedFd, ContainmentFailure> {
    // SAFETY: exact fixed host device path, no symlink following.
    let fd = unsafe {
        libc::open(
            c"/dev/null".as_ptr(),
            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if fd < 0 {
        return Err(io_platform(io::Error::last_os_error()));
    }
    // SAFETY: open returned one fresh descriptor.
    let original = unsafe { OwnedFd::from_raw_fd(fd) };
    let metadata = fd_stat(original.as_raw_fd()).map_err(|error| {
        ContainmentFailure::Platform(format!("verify fixed host /dev/null: {error}"))
    })?;
    if !is_exact_dev_null(&metadata) {
        return Err(ContainmentFailure::Platform(
            "fixed host /dev/null identity mismatch".to_string(),
        ));
    }
    duplicate_fd_above_fixed_range(original.as_raw_fd()).map_err(|error| {
        ContainmentFailure::Platform(format!("duplicate fixed host /dev/null: {error}"))
    })
}

fn fd_stat(fd: RawFd) -> Result<libc::stat, String> {
    let mut metadata: libc::stat = unsafe { mem::zeroed() };
    // SAFETY: fd is live and metadata is writable.
    if unsafe { libc::fstat(fd, &mut metadata) } != 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    Ok(metadata)
}

fn is_exact_dev_null(metadata: &libc::stat) -> bool {
    metadata.st_uid == 0
        && metadata.st_gid == 0
        && metadata.st_mode & libc::S_IFMT == libc::S_IFCHR
        && libc::major(metadata.st_rdev) == 1
        && libc::minor(metadata.st_rdev) == 3
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
    // The service-wide 0117 umask would strip both directory execute bits.
    // Clear it only in this already-cloned child while mkdir runs, then
    // restore it before observing the result. `/work` is an exact setgid
    // root:checker directory, so Linux supplies both the checker group and
    // S_ISGID at creation time without CAP_CHOWN or CAP_FSETID. A later
    // chmod(2) would clear that inherited S_ISGID because the launcher is not
    // a member of the checker group and deliberately lacks CAP_FSETID.
    // SAFETY: umask is private to this post-clone process, and the fixed path
    // lies inside the fresh per-request tmpfs.
    let previous_umask = unsafe { libc::umask(0) };
    let mkdir_result = unsafe { libc::mkdir(c"/work/scratch".as_ptr(), 0o770) };
    let mkdir_error = (mkdir_result != 0).then(io::Error::last_os_error);
    // SAFETY: restore the exact inherited launcher mask before any return or
    // untrusted execution.
    unsafe { libc::umask(previous_umask) };
    if let Some(error) = mkdir_error {
        return Err(error.to_string());
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
    // Preserve every source above the fixed 0..=3 target range before any
    // dup2/dup3 operation. This remains correct even when the parent arrived
    // with one or more standard descriptors already closed and pipe/open
    // allocation consequently reused 0..=3.
    let null_fd = unsafe { libc::open(c"/dev/null".as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC) };
    if null_fd < 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    // SAFETY: open returned one fresh descriptor.
    let null_fd = unsafe { OwnedFd::from_raw_fd(null_fd) };
    let null_source = duplicate_fd_above_fixed_range(null_fd.as_raw_fd())?;
    let stdout_source = duplicate_fd_above_fixed_range(stdout_fd)?;
    let stderr_source = duplicate_fd_above_fixed_range(stderr_fd)?;
    let setup_source = duplicate_fd_above_fixed_range(setup_fd)?;
    for (from, to) in [
        (null_source.as_raw_fd(), 0),
        (stdout_source.as_raw_fd(), 1),
        (stderr_source.as_raw_fd(), 2),
    ] {
        if unsafe { libc::dup2(from, to) } < 0 {
            return Err(io::Error::last_os_error().to_string());
        }
    }
    if unsafe { libc::dup3(setup_source.as_raw_fd(), 3, libc::O_CLOEXEC) } < 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    drop((
        null_fd,
        null_source,
        stdout_source,
        stderr_source,
        setup_source,
    ));
    // SAFETY: fd 0..=3 are the complete fixed inherited set; every higher
    // authority/control/ledger descriptor must disappear before untrusted exec.
    if unsafe { libc::syscall(libc::SYS_close_range, 4_u32, u32::MAX, 0_u32) } != 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    Ok(())
}

fn duplicate_fd_above_fixed_range(fd: RawFd) -> Result<OwnedFd, String> {
    // SAFETY: F_DUPFD_CLOEXEC duplicates one live descriptor into the first
    // unused number >= 4, independently of its original numeric value.
    let duplicate = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 4) };
    if duplicate < 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    // SAFETY: fcntl returned one fresh descriptor owned by this process.
    Ok(unsafe { OwnedFd::from_raw_fd(duplicate) })
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

fn verify_runtime_identity_lookup(uid: u32, gid: u32) -> Result<(), String> {
    let mut record: libc::passwd = unsafe { mem::zeroed() };
    let mut result: *mut libc::passwd = std::ptr::null_mut();
    let mut buffer = [0 as libc::c_char; 4096];
    // SAFETY: record, result and the fixed buffer remain writable for the
    // entire re-entrant NSS lookup. The dynamic passwd overlay was already
    // byte-verified before privileges were dropped.
    let status = unsafe {
        libc::getpwuid_r(
            uid,
            &mut record,
            buffer.as_mut_ptr(),
            buffer.len(),
            &mut result,
        )
    };
    if status != 0 {
        return Err(io::Error::from_raw_os_error(status).to_string());
    }
    if !std::ptr::eq(result, &record)
        || record.pw_uid != uid
        || record.pw_gid != gid
        || record.pw_name.is_null()
        || record.pw_passwd.is_null()
        || record.pw_gecos.is_null()
        || record.pw_dir.is_null()
        || record.pw_shell.is_null()
    {
        return Err("runtime checker identity lookup mismatch".to_string());
    }
    // SAFETY: a successful getpwuid_r result points each checked field into
    // the live buffer and terminates it with NUL.
    let fields_match = unsafe {
        CStr::from_ptr(record.pw_name) == c"boole-native-checker"
            && CStr::from_ptr(record.pw_passwd) == c"x"
            && CStr::from_ptr(record.pw_gecos) == c"boole-native-checker"
            && CStr::from_ptr(record.pw_dir) == c"/work/scratch"
            && CStr::from_ptr(record.pw_shell) == c"/usr/sbin/nologin"
    };
    if !fields_match {
        return Err("runtime checker identity fields mismatch".to_string());
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
        libc::SYS_swapoff,
        libc::SYS_swapon,
        libc::SYS_umount2,
        libc::SYS_unshare,
        libc::SYS_userfaultfd,
    ];
    let mut rules: std::collections::BTreeMap<i64, Vec<SeccompRule>> = syscalls
        .into_iter()
        .map(|number| (number, vec![]))
        .collect();
    // Rust's process launcher uses one anonymous AF_UNIX socketpair as its
    // close-on-exec error channel while Cargo starts rustc.  That pair cannot
    // name or connect to a network endpoint.  Keep every real socket() call
    // denied and deny socketpair() for every domain except this exact local
    // process-control primitive.
    let deny_non_local_socketpair = SeccompRule::new(vec![SeccompCondition::new(
        0,
        SeccompCmpArgLen::Dword,
        SeccompCmpOp::Ne,
        libc::AF_UNIX as u64,
    )
    .map_err(|error| ContainmentFailure::Platform(error.to_string()))?])
    .map_err(|error| ContainmentFailure::Platform(error.to_string()))?;
    rules.insert(libc::SYS_socketpair, vec![deny_non_local_socketpair]);
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
    // Python subprocess opens DEVNULL with O_RDWR before spawning cargo.  The
    // child-private /dev/null was already reopened, device-number verified,
    // and bind-mounted from the exact host character device.  Grant only that
    // one sink WRITE_FILE; `/dev` and every other rootfs path remain outside
    // the Landlock write allowlist.
    let dev_null = PathFd::new("/dev/null").map_err(|error| error.to_string())?;
    ruleset = ruleset
        .add_rule(PathBeneath::new(dev_null, AccessFs::WriteFile))
        .map_err(|error| error.to_string())?;
    let status = ruleset.restrict_self().map_err(|error| error.to_string())?;
    if status.ruleset != RulesetStatus::FullyEnforced || !status.no_new_privs {
        return Err("Landlock was not fully enforced".to_string());
    }
    Ok(())
}

fn set_checker_umask() -> Result<(), String> {
    // The root launcher deliberately starts under systemd's restrictive 0117
    // mask.  That mask cannot cross the exec boundary: Python tempfile asks
    // for 0700 directories, and 0700 & !0117 is 0600 (not traversable even by
    // the checker that owns it).  This post-clone child is single-threaded, so
    // replace only its mask with 0077.  Files remain owner-only and requested
    // 0700 scratch directories retain their owner execute bit.
    // SAFETY: umask is process-local in this already-cloned, single-threaded
    // child and the previous fixed launcher mask is checked exactly.
    let previous = unsafe { libc::umask(CHECKER_UMASK) };
    if previous != FIXED_LAUNCHER_UMASK {
        // SAFETY: restore the unexpected inherited value before failing setup.
        unsafe { libc::umask(previous) };
        return Err(format!(
            "inherited launcher umask mismatch: expected 0117, observed {previous:04o}"
        ));
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
    use std::collections::BTreeSet;
    use std::ffi::OsString;

    use sha2::{Digest, Sha256};

    use super::{
        build_seccomp_program, derived_runtime_top_level_is_exact,
        runtime_additions_are_compatible_with_lower, runtime_passwd_record,
        runtime_root_metadata_is_exact, setup_stage, CapturedOutput, OutputStream, CHECKER_UMASK,
        OUTPUT_LIMIT,
    };

    fn run_seccomp_probe(probe: fn() -> bool) -> i32 {
        let program = build_seccomp_program().expect("fixed seccomp program");
        // SAFETY: the child performs only seccomp installation, the supplied
        // raw-syscall probe, and _exit. The parent waits for exactly that PID.
        let pid = unsafe { libc::fork() };
        assert!(pid >= 0, "fork seccomp probe");
        if pid == 0 {
            let installed = seccompiler::apply_filter(&program).is_ok();
            let passed = installed && probe();
            // SAFETY: _exit terminates only this forked probe child without
            // running inherited Rust destructors.
            unsafe { libc::_exit(if passed { 0 } else { 1 }) };
        }
        let mut status = 0;
        // SAFETY: pid is the positive child PID returned by fork and status is
        // a valid writable wait-status location.
        assert_eq!(unsafe { libc::waitpid(pid, &mut status, 0) }, pid);
        assert!(libc::WIFEXITED(status), "seccomp probe did not exit");
        libc::WEXITSTATUS(status)
    }

    fn run_seccomp_spawn_probe() -> i32 {
        let program = build_seccomp_program().expect("fixed seccomp program");
        // SAFETY: the child installs the production filter, starts one fixed
        // harmless executable, and exits with a categorical status. The
        // parent waits for exactly that PID and never consumes child output.
        let pid = unsafe { libc::fork() };
        assert!(pid >= 0, "fork seccomp process-spawn probe");
        if pid == 0 {
            let status = match seccompiler::apply_filter(&program) {
                Ok(()) => match std::process::Command::new("/bin/true").status() {
                    Ok(status) if status.success() => 0,
                    Err(error) if error.raw_os_error() == Some(libc::EACCES) => libc::EACCES,
                    _ => 2,
                },
                Err(_) => 3,
            };
            // SAFETY: _exit terminates only this forked probe child without
            // running inherited Rust destructors.
            unsafe { libc::_exit(status) };
        }
        let mut status = 0;
        // SAFETY: pid is the positive child PID returned by fork and status is
        // a valid writable wait-status location.
        assert_eq!(unsafe { libc::waitpid(pid, &mut status, 0) }, pid);
        assert!(libc::WIFEXITED(status), "process-spawn probe did not exit");
        libc::WEXITSTATUS(status)
    }

    fn local_socketpair_succeeds() -> bool {
        let mut fds = [-1; 2];
        // SAFETY: fds has space for exactly two descriptors. The fixed local
        // domain/type/protocol tuple creates no named or network endpoint.
        let result = unsafe {
            libc::socketpair(
                libc::AF_UNIX,
                libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC,
                0,
                fds.as_mut_ptr(),
            )
        };
        if result != 0 {
            return false;
        }
        // SAFETY: successful socketpair initialized both descriptors.
        unsafe {
            libc::close(fds[0]);
            libc::close(fds[1]);
        }
        true
    }

    fn non_local_socketpair_is_eacces() -> bool {
        let mut fds = [-1; 2];
        // SAFETY: errno is thread-local in this forked single-thread probe and
        // fds has space for the kernel's two output descriptors.
        unsafe {
            *libc::__errno_location() = 0;
            libc::socketpair(
                libc::AF_INET,
                libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC,
                0,
                fds.as_mut_ptr(),
            ) == -1
                && *libc::__errno_location() == libc::EACCES
        }
    }

    fn socket_remains_eacces() -> bool {
        // SAFETY: this deliberately probes the fixed seccomp denial. No valid
        // descriptor is returned when the expected EACCES result occurs.
        unsafe {
            *libc::__errno_location() = 0;
            libc::socket(libc::AF_UNIX, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0) == -1
                && *libc::__errno_location() == libc::EACCES
        }
    }

    fn local_spawn_channel_recv_is_eacces() -> bool {
        let mut fds = [-1; 2];
        // SAFETY: fds has room for the anonymous local pair. Closing the peer
        // makes an allowed recv return EOF immediately instead of blocking.
        unsafe {
            if libc::socketpair(
                libc::AF_UNIX,
                libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC,
                0,
                fds.as_mut_ptr(),
            ) != 0
            {
                return false;
            }
            libc::close(fds[1]);
            *libc::__errno_location() = 0;
            let mut byte = 0_u8;
            let denied = libc::recv(fds[0], (&mut byte as *mut u8).cast(), 1, 0) == -1
                && *libc::__errno_location() == libc::EACCES;
            libc::close(fds[0]);
            denied
        }
    }

    fn local_spawn_channel_send_is_eacces() -> bool {
        let mut fds = [-1; 2];
        // SAFETY: fds has room for the anonymous local pair and the one-byte
        // send cannot escape that unnamed pair.
        unsafe {
            if libc::socketpair(
                libc::AF_UNIX,
                libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC,
                0,
                fds.as_mut_ptr(),
            ) != 0
            {
                return false;
            }
            *libc::__errno_location() = 0;
            let byte = 0_u8;
            let denied = libc::send(fds[0], (&byte as *const u8).cast(), 1, 0) == -1
                && *libc::__errno_location() == libc::EACCES;
            libc::close(fds[0]);
            libc::close(fds[1]);
            denied
        }
    }

    fn names(values: &[&str]) -> BTreeSet<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn fixed_capability_child_uses_same_dot_pivot_without_an_old_root_path() {
        let source = include_str!("linux.rs");
        assert!(source
            .contains("libc::syscall(libc::SYS_pivot_root, c\".\".as_ptr(), c\".\".as_ptr())"));
        assert!(source.contains("libc::umount2(c\".\".as_ptr(), libc::MNT_DETACH)"));
        let old_root_path = [".old", "-root"].concat();
        let remove_directory_call = ["libc::", "rmdir"].concat();
        let chroot_call = ["libc::", "chroot("].concat();
        assert!(!source.contains(&old_root_path));
        assert!(!source.contains(&remove_directory_call));
        assert!(
            !source.contains(&chroot_call),
            "CAP_SYS_CHROOT is deliberately absent from the fixed launcher capability set"
        );
    }

    #[test]
    fn derived_runtime_root_adds_one_reviewed_etc_shadow_and_four_mountpoints() {
        let lower = names(&["etc", "opt", "usr"]);
        let exact = names(&["dev", "etc", "opt", "proc", "tmp", "usr", "work"]);
        assert!(runtime_additions_are_compatible_with_lower(&lower));
        assert!(derived_runtime_top_level_is_exact(&lower, &exact));

        let extra = names(&["dev", "etc", "host", "opt", "proc", "tmp", "usr", "work"]);
        assert!(!derived_runtime_top_level_is_exact(&lower, &extra));

        let conflicting_lower = names(&["etc", "proc", "usr"]);
        assert!(!runtime_additions_are_compatible_with_lower(
            &conflicting_lower
        ));
        assert!(!derived_runtime_top_level_is_exact(
            &conflicting_lower,
            &exact
        ));

        let missing_reviewed_overlap = names(&["opt", "usr"]);
        assert!(!runtime_additions_are_compatible_with_lower(
            &missing_reviewed_overlap
        ));
    }

    #[test]
    fn runtime_passwd_record_is_fixed_and_uses_only_the_numeric_checker_identity() {
        assert_eq!(
            runtime_passwd_record(42001, 42002).expect("non-root checker identity"),
            b"boole-native-checker:x:42001:42002:boole-native-checker:/work/scratch:/usr/sbin/nologin\n"
        );
        assert!(runtime_passwd_record(0, 42002).is_err());
        assert!(runtime_passwd_record(42001, 0).is_err());
    }

    #[test]
    fn derived_runtime_root_is_traversable_after_checker_privilege_drop() {
        assert!(runtime_root_metadata_is_exact(0, 0, libc::S_IFDIR | 0o755));
        assert!(!runtime_root_metadata_is_exact(0, 0, libc::S_IFDIR | 0o700));
        assert!(!runtime_root_metadata_is_exact(1, 0, libc::S_IFDIR | 0o755));
    }

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

    #[test]
    fn child_setup_errors_name_the_exact_failed_stage() {
        assert_eq!(
            setup_stage::<()>(
                "install-landlock",
                Err("Permission denied (os error 13)".to_string())
            ),
            Err("install-landlock: Permission denied (os error 13)".to_string())
        );
    }

    #[test]
    fn checker_umask_preserves_owner_traversal_and_removes_all_peer_access() {
        assert_eq!(0o700 & !CHECKER_UMASK, 0o700);
        assert_eq!(0o666 & !CHECKER_UMASK, 0o600);
        assert_eq!(CHECKER_UMASK, 0o077);

        let source = include_str!("linux.rs");
        let install = source
            .find("set-checker-umask")
            .expect("checker umask setup stage");
        let execute = source.find("exec-checker").expect("checker exec stage");
        assert!(install < execute);
    }

    #[test]
    fn seccomp_allows_only_anonymous_local_socketpairs_for_process_spawning() {
        assert_eq!(run_seccomp_probe(local_socketpair_succeeds), 0);
        assert_eq!(run_seccomp_probe(non_local_socketpair_is_eacces), 0);
        assert_eq!(run_seccomp_probe(socket_remains_eacces), 0);
    }

    #[test]
    fn seccomp_preserves_the_rust_process_spawn_control_channel() {
        if std::env::var_os("BOOLE_NATIVE_SHADOW_SECCOMP_SPAWN_PROBE").is_none() {
            return;
        }
        let spawn_status = run_seccomp_spawn_probe();
        let recv_eacces = run_seccomp_probe(local_spawn_channel_recv_is_eacces) == 0;
        let send_eacces = run_seccomp_probe(local_spawn_channel_send_is_eacces) == 0;
        assert_eq!(
            spawn_status,
            0,
            "Rust process spawn failed under the production filter; \
             categorical evidence: spawn_eacces={}, recv_eacces={recv_eacces}, \
             send_eacces={send_eacces}",
            spawn_status == libc::EACCES,
        );
    }
}
