//! Descriptor-relative cgroup v2 operations shared by launcher startup and
//! later orphan recovery.

#![cfg(target_os = "linux")]

use std::collections::BTreeSet;
use std::ffi::{CStr, CString};
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::mem::MaybeUninit;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::OpenOptionsExt;

use thiserror::Error;

const FIXED_PATH_COMPONENTS: [&str; 5] = [
    "sys",
    "fs",
    "cgroup",
    "system.slice",
    "boole-native-shadow-launcher.service",
];
const MANAGER_BASENAME: &str = "manager";
const MANAGER_DIRECTORY_MODE: libc::mode_t = 0o700;
const CGROUP2_SUPER_MAGIC: libc::c_long = 0x6367_7270;
const MAX_CONTROL_BYTES: u64 = 65_536;

#[derive(Debug, Error)]
pub(crate) enum CgroupFsError {
    #[error("cgroup I/O failed during {operation}: {source}")]
    Io {
        operation: &'static str,
        #[source]
        source: io::Error,
    },
    #[error("unsafe cgroup state: {0}")]
    UnsafeState(String),
    #[error("the startup cgroup cleanup deadline expired")]
    DeadlineExceeded,
}

#[derive(Debug)]
pub(crate) struct CgroupDirectory {
    file: File,
}

#[derive(Debug)]
pub(crate) struct RecoveryLeaf {
    directory: CgroupDirectory,
    basename: String,
    identity: DirectoryIdentity,
}

#[allow(dead_code)] // Becomes live when the active-execution service is wired.
#[derive(Debug)]
pub(crate) struct ExecutionLeaf {
    directory: CgroupDirectory,
    basename: String,
    identity: DirectoryIdentity,
}

#[allow(dead_code)]
impl ExecutionLeaf {
    pub(crate) fn raw_fd(&self) -> std::os::fd::RawFd {
        self.directory.file.as_raw_fd()
    }
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct ExecutionResourceSnapshot {
    pub(crate) cpu_usage_usec: u64,
    pub(crate) memory_peak_bytes: u64,
    pub(crate) memory_events_low: u64,
    pub(crate) memory_events_high: u64,
    pub(crate) memory_events_max: u64,
    pub(crate) memory_events_oom: u64,
    pub(crate) memory_events_oom_kill: u64,
    pub(crate) memory_events_oom_group_kill: u64,
    pub(crate) pids_events_max: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DirectoryIdentity {
    device: libc::dev_t,
    inode: libc::ino_t,
}

pub(crate) fn open_fixed_service_root() -> Result<CgroupDirectory, CgroupFsError> {
    let mut current = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open("/")
        .map_err(|source| io_error("open filesystem root", source))?;

    for component in FIXED_PATH_COMPONENTS {
        current = open_directory_child(&current, component, "open fixed service cgroup path")?;
    }

    require_cgroup2fs(&current)?;
    Ok(CgroupDirectory { file: current })
}

#[allow(unsafe_code)]
pub(crate) fn open_or_create_manager(
    root: &CgroupDirectory,
) -> Result<CgroupDirectory, CgroupFsError> {
    let basename = CString::new(MANAGER_BASENAME).expect("fixed manager basename has no NUL");
    // SAFETY: `root` is a live directory descriptor and `basename` is a
    // fixed, NUL-terminated direct-child name.
    let created = unsafe {
        libc::mkdirat(
            root.file.as_raw_fd(),
            basename.as_ptr(),
            MANAGER_DIRECTORY_MODE,
        )
    };
    let was_created = created == 0;
    if created != 0 {
        let source = io::Error::last_os_error();
        if source.raw_os_error() != Some(libc::EEXIST) {
            return Err(io_error("create manager cgroup", source));
        }
    }

    let file = open_directory_child(&root.file, MANAGER_BASENAME, "open manager cgroup")?;
    if was_created {
        set_directory_mode(&file, MANAGER_DIRECTORY_MODE)?;
    }
    require_manager_metadata(&file)?;
    require_cgroup2fs(&file)?;
    Ok(CgroupDirectory { file })
}

pub(crate) fn verify_manager_empty_before_move(
    manager: &CgroupDirectory,
) -> Result<(), CgroupFsError> {
    let events = read_control(manager, "cgroup.events")?;
    let cgroup_type = read_control(manager, "cgroup.type")?;
    require_reusable_manager_state(&events, &cgroup_type)?;
    require_empty_subtree_control(&read_control(manager, "cgroup.subtree_control")?)?;
    require_empty_id_file(manager, "cgroup.procs")?;
    require_empty_id_file(manager, "cgroup.threads")?;
    require_no_child_cgroups(manager, "manager cgroup has nested children before reuse")
}

fn require_reusable_manager_state(events: &str, cgroup_type: &str) -> Result<(), CgroupFsError> {
    let populated = parse_required_counter(events, "populated")?;
    if populated != 0 {
        return Err(unsafe_state("manager cgroup is populated before reuse"));
    }
    let frozen = parse_required_counter(events, "frozen")?;
    if frozen != 0 {
        return Err(unsafe_state("manager cgroup is frozen before reuse"));
    }
    require_exact_domain_type(cgroup_type)
}

fn require_exact_domain_type(cgroup_type: &str) -> Result<(), CgroupFsError> {
    if cgroup_type.split_whitespace().collect::<Vec<_>>() != ["domain"] {
        return Err(unsafe_state("manager cgroup type is not exact domain"));
    }
    Ok(())
}

fn require_empty_subtree_control(text: &str) -> Result<(), CgroupFsError> {
    if text.split_whitespace().next().is_none() {
        Ok(())
    } else {
        Err(unsafe_state(
            "manager cgroup.subtree_control is not exact empty",
        ))
    }
}

pub(crate) fn move_current_process_into_manager(
    manager: &CgroupDirectory,
) -> Result<(), CgroupFsError> {
    let pid = current_pid()?;
    write_control(manager, "cgroup.procs", &format!("{pid}\n"))
}

pub(crate) fn verify_service_root_has_no_processes(
    root: &CgroupDirectory,
) -> Result<(), CgroupFsError> {
    require_empty_id_file(root, "cgroup.procs")
}

pub(crate) fn verify_service_root_descriptor(root: &CgroupDirectory) -> Result<(), CgroupFsError> {
    require_cgroup2fs(&root.file)
}

pub(crate) fn enable_required_controllers(root: &CgroupDirectory) -> Result<(), CgroupFsError> {
    write_control(root, "cgroup.subtree_control", "+cpu +memory +pids\n")
}

pub(crate) fn verify_required_controllers(root: &CgroupDirectory) -> Result<(), CgroupFsError> {
    let actual = parse_words(&read_control(root, "cgroup.subtree_control")?)?;
    let expected = BTreeSet::from(["cpu".to_string(), "memory".to_string(), "pids".to_string()]);
    if actual != expected {
        return Err(unsafe_state(format!(
            "service subtree controllers differ: expected {expected:?}, actual {actual:?}"
        )));
    }
    Ok(())
}

pub(crate) fn verify_manager_after_move(manager: &CgroupDirectory) -> Result<(), CgroupFsError> {
    require_exact_domain_type(&read_control(manager, "cgroup.type")?)?;
    require_empty_subtree_control(&read_control(manager, "cgroup.subtree_control")?)?;
    let pid = current_pid()?;
    let tid = current_tid()?;
    let procs = parse_id_file(manager, "cgroup.procs")?;
    if procs != [pid] {
        return Err(unsafe_state(format!(
            "manager cgroup.procs differs after move: expected [{pid}], actual {procs:?}"
        )));
    }
    let threads = parse_id_file(manager, "cgroup.threads")?;
    if threads != [tid] {
        return Err(unsafe_state(format!(
            "manager cgroup.threads differs after move: expected [{tid}], actual {threads:?}"
        )));
    }
    require_no_child_cgroups(manager, "manager cgroup has nested children after move")
}

pub(crate) fn verify_manager_descriptor(manager: &CgroupDirectory) -> Result<(), CgroupFsError> {
    require_cgroup2fs(&manager.file)?;
    require_manager_metadata(&manager.file)
}

#[allow(dead_code, unsafe_code)]
pub(crate) fn create_execution_leaf(
    root: &CgroupDirectory,
    basename: &str,
) -> Result<ExecutionLeaf, CgroupFsError> {
    if !is_exact_run_leaf_name(basename) {
        return Err(unsafe_state(
            "execution leaf basename is not exact run-<operationId>",
        ));
    }
    let basename_c =
        CString::new(basename).map_err(|_| unsafe_state("execution leaf basename contains NUL"))?;
    // SAFETY: `root` is a live descriptor and `basename_c` is one validated,
    // NUL-terminated direct-child component.
    if unsafe { libc::mkdirat(root.file.as_raw_fd(), basename_c.as_ptr(), 0o700) } != 0 {
        return Err(io_error(
            "create execution cgroup leaf",
            io::Error::last_os_error(),
        ));
    }

    let result = (|| {
        let file = open_directory_child(&root.file, basename, "open execution cgroup leaf")?;
        set_directory_mode(&file, 0o700)?;
        require_root_directory_metadata(&file, 0o700, "execution cgroup leaf")?;
        require_cgroup2fs(&file)?;
        let directory = CgroupDirectory { file };
        require_exact_domain_type(&read_control(&directory, "cgroup.type")?)?;
        require_empty_subtree_control(&read_control(&directory, "cgroup.subtree_control")?)?;
        require_no_child_cgroups(&directory, "execution leaf has nested cgroups")?;
        require_empty_id_file(&directory, "cgroup.procs")?;
        require_empty_id_file(&directory, "cgroup.threads")?;
        let events = read_control(&directory, "cgroup.events")?;
        if require_binary_event(&events, "populated")? != 0
            || require_binary_event(&events, "frozen")? != 0
        {
            return Err(unsafe_state("new execution leaf is populated or frozen"));
        }
        let identity = descriptor_identity(&directory.file, "identify execution cgroup leaf")?;
        Ok(ExecutionLeaf {
            directory,
            basename: basename.to_string(),
            identity,
        })
    })();

    if result.is_err() {
        // No process can enter before clone3, so direct removal is the only
        // safe rollback for a partially configured new leaf.
        // SAFETY: the name remains the exact direct child created above.
        let _ = unsafe {
            libc::unlinkat(
                root.file.as_raw_fd(),
                basename_c.as_ptr(),
                libc::AT_REMOVEDIR,
            )
        };
    }
    result
}

#[allow(dead_code)]
pub(crate) fn apply_execution_leaf_limits(leaf: &ExecutionLeaf) -> Result<(), CgroupFsError> {
    for (name, value) in [
        ("pids.max", "128\n"),
        ("memory.max", "2147483648\n"),
        ("memory.swap.max", "0\n"),
        ("memory.oom.group", "1\n"),
        ("cpu.max", "max 100000\n"),
    ] {
        write_control(&leaf.directory, name, value)?;
    }
    for (name, expected) in [
        ("pids.max", "128"),
        ("memory.max", "2147483648"),
        ("memory.swap.max", "0"),
        ("memory.oom.group", "1"),
        ("cpu.max", "max 100000"),
    ] {
        if read_control(&leaf.directory, name)?.trim() != expected {
            return Err(unsafe_state(format!(
                "execution leaf {name} readback mismatch"
            )));
        }
    }
    drop(open_control(
        &leaf.directory.file,
        "cgroup.freeze",
        libc::O_WRONLY,
    )?);
    drop(open_control(
        &leaf.directory.file,
        "cgroup.kill",
        libc::O_WRONLY,
    )?);
    Ok(())
}

#[allow(dead_code)]
pub(crate) fn read_execution_resources(
    leaf: &ExecutionLeaf,
) -> Result<ExecutionResourceSnapshot, CgroupFsError> {
    let cpu = read_control(&leaf.directory, "cpu.stat")?;
    let memory = read_control(&leaf.directory, "memory.events")?;
    let pids = read_control(&leaf.directory, "pids.events")?;
    let memory_peak_bytes = read_control(&leaf.directory, "memory.peak")?
        .trim()
        .parse::<u64>()
        .map_err(|_| unsafe_state("memory.peak is malformed"))?;
    Ok(ExecutionResourceSnapshot {
        cpu_usage_usec: parse_required_counter(&cpu, "usage_usec")?,
        memory_peak_bytes,
        memory_events_low: parse_required_counter(&memory, "low")?,
        memory_events_high: parse_required_counter(&memory, "high")?,
        memory_events_max: parse_required_counter(&memory, "max")?,
        memory_events_oom: parse_required_counter(&memory, "oom")?,
        memory_events_oom_kill: parse_required_counter(&memory, "oom_kill")?,
        memory_events_oom_group_kill: parse_required_counter(&memory, "oom_group_kill")?,
        pids_events_max: parse_required_counter(&pids, "max")?,
    })
}

#[allow(dead_code)]
pub(crate) fn freeze_execution_leaf(leaf: &ExecutionLeaf) -> Result<(), CgroupFsError> {
    write_control(&leaf.directory, "cgroup.freeze", "1\n")
}

#[allow(dead_code)]
pub(crate) fn kill_execution_leaf(leaf: &ExecutionLeaf) -> Result<(), CgroupFsError> {
    write_control(&leaf.directory, "cgroup.kill", "1\n")
}

#[allow(dead_code)]
pub(crate) fn wait_execution_leaf_event(
    leaf: &ExecutionLeaf,
    key: &'static str,
    expected: u64,
    deadline: std::time::Instant,
) -> Result<(), CgroupFsError> {
    const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(5);
    loop {
        if std::time::Instant::now() >= deadline {
            return Err(CgroupFsError::DeadlineExceeded);
        }
        let observed =
            parse_required_counter(&read_control(&leaf.directory, "cgroup.events")?, key)?;
        if observed == expected {
            return Ok(());
        }
        std::thread::sleep(
            POLL_INTERVAL.min(deadline.saturating_duration_since(std::time::Instant::now())),
        );
    }
}

#[allow(dead_code)]
pub(crate) fn read_execution_leaf_event(
    leaf: &ExecutionLeaf,
    key: &'static str,
) -> Result<u64, CgroupFsError> {
    require_binary_event(&read_control(&leaf.directory, "cgroup.events")?, key)
}

#[allow(dead_code)]
pub(crate) fn verify_execution_leaf_ids_empty(leaf: &ExecutionLeaf) -> Result<(), CgroupFsError> {
    require_empty_id_file(&leaf.directory, "cgroup.procs")?;
    require_empty_id_file(&leaf.directory, "cgroup.threads")
}

#[allow(unsafe_code)]
#[allow(dead_code)]
pub(crate) fn remove_execution_leaf(
    root: &CgroupDirectory,
    leaf: ExecutionLeaf,
) -> Result<(), CgroupFsError> {
    let basename = CString::new(leaf.basename.as_str())
        .map_err(|_| unsafe_state("execution leaf basename contains NUL"))?;
    if child_identity(&root.file, &basename)? != leaf.identity {
        return Err(unsafe_state(
            "execution leaf identity changed before removal",
        ));
    }
    // SAFETY: the validated descriptor and identity remain live through the
    // direct-child removal.
    if unsafe { libc::unlinkat(root.file.as_raw_fd(), basename.as_ptr(), libc::AT_REMOVEDIR) } != 0
    {
        return Err(io_error(
            "remove execution cgroup leaf",
            io::Error::last_os_error(),
        ));
    }
    drop(leaf);
    Ok(())
}

#[allow(dead_code)]
fn is_exact_run_leaf_name(value: &str) -> bool {
    let Some(payload) = value.strip_prefix("run-") else {
        return false;
    };
    payload.len() == 64
        && payload
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(crate) fn scan_service_child_cgroups(
    root: &CgroupDirectory,
) -> Result<Vec<String>, CgroupFsError> {
    direct_child_directories(&root.file)
}

pub(crate) fn open_and_validate_recovery_leaf(
    root: &CgroupDirectory,
    basename: &str,
) -> Result<RecoveryLeaf, CgroupFsError> {
    let file = open_directory_child(&root.file, basename, "open startup recovery leaf")?;
    let identity = descriptor_identity(&file, "identify startup recovery leaf")?;
    require_cgroup2fs(&file)?;
    let directory = CgroupDirectory { file };
    require_exact_domain_type(&read_control(&directory, "cgroup.type")?)?;
    require_empty_subtree_control(&read_control(&directory, "cgroup.subtree_control")?)?;
    require_no_child_cgroups(&directory, "startup recovery leaf has nested child cgroups")?;
    require_binary_event(&read_control(&directory, "cgroup.events")?, "populated")?;
    require_binary_event(&read_control(&directory, "cgroup.events")?, "frozen")?;
    let _ = parse_id_file(&directory, "cgroup.procs")?;
    let _ = parse_id_file(&directory, "cgroup.threads")?;
    drop(open_control(
        &directory.file,
        "cgroup.freeze",
        libc::O_WRONLY,
    )?);
    drop(open_control(
        &directory.file,
        "cgroup.kill",
        libc::O_WRONLY,
    )?);
    Ok(RecoveryLeaf {
        directory,
        basename: basename.to_string(),
        identity,
    })
}

pub(crate) fn freeze_recovery_leaf(leaf: &RecoveryLeaf) -> Result<(), CgroupFsError> {
    write_control(&leaf.directory, "cgroup.freeze", "1\n")
}

pub(crate) fn kill_recovery_leaf(leaf: &RecoveryLeaf) -> Result<(), CgroupFsError> {
    write_control(&leaf.directory, "cgroup.kill", "1\n")
}

pub(crate) fn wait_recovery_leaf_event(
    leaf: &RecoveryLeaf,
    key: &'static str,
    expected: u64,
    deadline: std::time::Instant,
) -> Result<(), CgroupFsError> {
    const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(5);
    loop {
        if std::time::Instant::now() >= deadline {
            return Err(CgroupFsError::DeadlineExceeded);
        }
        let observed =
            parse_required_counter(&read_control(&leaf.directory, "cgroup.events")?, key)?;
        let now = std::time::Instant::now();
        if now >= deadline {
            return Err(CgroupFsError::DeadlineExceeded);
        }
        if observed == expected {
            return Ok(());
        }
        std::thread::sleep(POLL_INTERVAL.min(deadline.saturating_duration_since(now)));
    }
}

pub(crate) fn verify_recovery_leaf_ids_empty(leaf: &RecoveryLeaf) -> Result<(), CgroupFsError> {
    require_empty_id_file(&leaf.directory, "cgroup.procs")?;
    require_empty_id_file(&leaf.directory, "cgroup.threads")
}

#[allow(unsafe_code)]
pub(crate) fn remove_recovery_leaf(
    root: &CgroupDirectory,
    leaf: RecoveryLeaf,
) -> Result<(), CgroupFsError> {
    let basename = CString::new(leaf.basename.as_str())
        .map_err(|_| unsafe_state("startup recovery leaf basename contains NUL"))?;
    let current_identity = child_identity(&root.file, &basename)?;
    if current_identity != leaf.identity {
        return Err(unsafe_state(
            "startup recovery leaf identity changed before removal",
        ));
    }
    // Keep the validated leaf descriptor alive through removal. The caller
    // owns the fixed delegated root and has already completed a full
    // inventory pass before any mutation.
    let result =
        unsafe { libc::unlinkat(root.file.as_raw_fd(), basename.as_ptr(), libc::AT_REMOVEDIR) };
    if result != 0 {
        return Err(io_error(
            "remove startup recovery leaf",
            io::Error::last_os_error(),
        ));
    }
    drop(leaf);
    Ok(())
}

fn require_empty_id_file(
    directory: &CgroupDirectory,
    basename: &'static str,
) -> Result<(), CgroupFsError> {
    let values = parse_id_file(directory, basename)?;
    if values.is_empty() {
        Ok(())
    } else {
        Err(unsafe_state(format!(
            "{basename} must be empty, found {values:?}"
        )))
    }
}

fn parse_id_file(
    directory: &CgroupDirectory,
    basename: &'static str,
) -> Result<Vec<u32>, CgroupFsError> {
    read_control(directory, basename)?
        .split_whitespace()
        .map(|value| {
            value
                .parse::<u32>()
                .map_err(|_| unsafe_state(format!("{basename} contains a malformed ID")))
        })
        .collect()
}

fn parse_required_counter(text: &str, required: &str) -> Result<u64, CgroupFsError> {
    let mut found = None;
    for line in text.lines() {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.first().copied() != Some(required) {
            continue;
        }
        if fields.len() != 2 || found.is_some() {
            return Err(unsafe_state(format!(
                "cgroup.events has malformed or duplicate {required}"
            )));
        }
        found = Some(
            fields[1]
                .parse::<u64>()
                .map_err(|_| unsafe_state(format!("cgroup.events has malformed {required}")))?,
        );
    }
    found.ok_or_else(|| unsafe_state(format!("cgroup.events is missing {required}")))
}

fn require_binary_event(text: &str, required: &str) -> Result<u64, CgroupFsError> {
    let value = parse_required_counter(text, required)?;
    if value <= 1 {
        Ok(value)
    } else {
        Err(unsafe_state(format!(
            "cgroup.events has non-binary {required}"
        )))
    }
}

fn parse_words(text: &str) -> Result<BTreeSet<String>, CgroupFsError> {
    let words = text
        .split_whitespace()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let unique = words.iter().cloned().collect::<BTreeSet<_>>();
    if words.len() != unique.len() {
        return Err(unsafe_state("controller read-back contains duplicates"));
    }
    Ok(unique)
}

fn require_no_child_cgroups(
    directory: &CgroupDirectory,
    reason: &'static str,
) -> Result<(), CgroupFsError> {
    let children = direct_child_directories(&directory.file)?;
    if children.is_empty() {
        Ok(())
    } else {
        Err(unsafe_state(format!("{reason}: {children:?}")))
    }
}

fn read_control(
    directory: &CgroupDirectory,
    basename: &'static str,
) -> Result<String, CgroupFsError> {
    let mut file = open_control(&directory.file, basename, libc::O_RDONLY)?;
    let mut bytes = Vec::new();
    (&mut file)
        .take(MAX_CONTROL_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| io_error("read cgroup control", source))?;
    if bytes.len() as u64 > MAX_CONTROL_BYTES {
        return Err(unsafe_state(format!("{basename} exceeds read ceiling")));
    }
    String::from_utf8(bytes).map_err(|_| unsafe_state(format!("{basename} is not valid UTF-8")))
}

fn write_control(
    directory: &CgroupDirectory,
    basename: &'static str,
    value: &str,
) -> Result<(), CgroupFsError> {
    let mut file = open_control(&directory.file, basename, libc::O_WRONLY)?;
    file.write_all(value.as_bytes())
        .map_err(|source| io_error("write cgroup control", source))
}

#[allow(unsafe_code)]
fn open_control(
    directory: &File,
    basename: &'static str,
    access: libc::c_int,
) -> Result<File, CgroupFsError> {
    let basename = CString::new(basename).expect("fixed control basename has no NUL");
    let flags = access | libc::O_CLOEXEC | libc::O_NOFOLLOW;
    // SAFETY: `directory` is live, the fixed basename is NUL-terminated, and
    // ownership of the returned descriptor moves exactly once into `File`.
    let raw_fd = unsafe { libc::openat(directory.as_raw_fd(), basename.as_ptr(), flags) };
    if raw_fd < 0 {
        return Err(io_error("open cgroup control", io::Error::last_os_error()));
    }
    // SAFETY: `openat` returned a fresh descriptor owned by this function.
    Ok(unsafe { File::from_raw_fd(raw_fd) })
}

#[allow(unsafe_code)]
fn open_directory_child(
    parent: &File,
    basename: &str,
    operation: &'static str,
) -> Result<File, CgroupFsError> {
    let basename = CString::new(basename).expect("fixed cgroup component has no NUL");
    let flags = libc::O_RDONLY | libc::O_CLOEXEC | libc::O_DIRECTORY | libc::O_NOFOLLOW;
    // SAFETY: `parent` is live, the component is fixed and NUL-terminated,
    // and ownership of the returned descriptor moves once into `File`.
    let raw_fd = unsafe { libc::openat(parent.as_raw_fd(), basename.as_ptr(), flags) };
    if raw_fd < 0 {
        return Err(io_error(operation, io::Error::last_os_error()));
    }
    // SAFETY: `openat` returned a fresh descriptor owned by this function.
    Ok(unsafe { File::from_raw_fd(raw_fd) })
}

#[allow(unsafe_code)]
fn set_directory_mode(file: &File, mode: libc::mode_t) -> Result<(), CgroupFsError> {
    // SAFETY: `file` is a live descriptor and `mode` contains only the fixed
    // permission bits frozen by the manager-cgroup contract.
    if unsafe { libc::fchmod(file.as_raw_fd(), mode) } != 0 {
        return Err(io_error(
            "set manager cgroup directory mode",
            io::Error::last_os_error(),
        ));
    }
    Ok(())
}

#[allow(unsafe_code)]
fn require_manager_metadata(file: &File) -> Result<(), CgroupFsError> {
    let mut stat = MaybeUninit::<libc::stat>::uninit();
    // SAFETY: `stat` is writable output storage and `file` remains live.
    if unsafe { libc::fstat(file.as_raw_fd(), stat.as_mut_ptr()) } != 0 {
        return Err(io_error(
            "verify manager cgroup metadata",
            io::Error::last_os_error(),
        ));
    }
    // SAFETY: successful `fstat` initialized the full structure.
    let stat = unsafe { stat.assume_init() };
    let mode = stat.st_mode & 0o7777;
    if stat.st_uid != 0 || stat.st_gid != 0 || mode != MANAGER_DIRECTORY_MODE {
        return Err(unsafe_state(format!(
            "manager cgroup metadata differs: expected uid=0 gid=0 mode={MANAGER_DIRECTORY_MODE:#05o}, actual uid={} gid={} mode={mode:#05o}",
            stat.st_uid, stat.st_gid
        )));
    }
    Ok(())
}

#[allow(unsafe_code)]
#[allow(dead_code)]
fn require_root_directory_metadata(
    file: &File,
    required_mode: libc::mode_t,
    label: &'static str,
) -> Result<(), CgroupFsError> {
    let mut stat = MaybeUninit::<libc::stat>::uninit();
    // SAFETY: `stat` is writable output storage and `file` remains live.
    if unsafe { libc::fstat(file.as_raw_fd(), stat.as_mut_ptr()) } != 0 {
        return Err(io_error(
            "verify cgroup directory metadata",
            io::Error::last_os_error(),
        ));
    }
    // SAFETY: successful `fstat` initialized the structure.
    let stat = unsafe { stat.assume_init() };
    let mode = stat.st_mode & 0o7777;
    if stat.st_uid != 0 || stat.st_gid != 0 || mode != required_mode {
        return Err(unsafe_state(format!(
            "{label} metadata differs: expected uid=0 gid=0 mode={required_mode:#05o}, actual uid={} gid={} mode={mode:#05o}",
            stat.st_uid, stat.st_gid
        )));
    }
    Ok(())
}

#[allow(unsafe_code)]
fn descriptor_identity(
    file: &File,
    operation: &'static str,
) -> Result<DirectoryIdentity, CgroupFsError> {
    let mut stat = MaybeUninit::<libc::stat>::uninit();
    // SAFETY: `stat` is writable output storage and `file` remains live.
    if unsafe { libc::fstat(file.as_raw_fd(), stat.as_mut_ptr()) } != 0 {
        return Err(io_error(operation, io::Error::last_os_error()));
    }
    // SAFETY: successful `fstat` initialized the full structure.
    let stat = unsafe { stat.assume_init() };
    Ok(DirectoryIdentity {
        device: stat.st_dev,
        inode: stat.st_ino,
    })
}

#[allow(unsafe_code)]
fn child_identity(parent: &File, basename: &CString) -> Result<DirectoryIdentity, CgroupFsError> {
    let mut stat = MaybeUninit::<libc::stat>::uninit();
    // SAFETY: `parent` is live, `basename` is NUL-terminated, and `stat` is
    // writable output storage.
    if unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            basename.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } != 0
    {
        return Err(io_error(
            "re-identify startup recovery leaf",
            io::Error::last_os_error(),
        ));
    }
    // SAFETY: successful `fstatat` initialized the full structure.
    let stat = unsafe { stat.assume_init() };
    if stat.st_mode & libc::S_IFMT != libc::S_IFDIR {
        return Err(unsafe_state(
            "startup recovery leaf name no longer resolves to a directory",
        ));
    }
    Ok(DirectoryIdentity {
        device: stat.st_dev,
        inode: stat.st_ino,
    })
}

#[allow(unsafe_code)]
fn require_cgroup2fs(directory: &File) -> Result<(), CgroupFsError> {
    let mut stat = MaybeUninit::<libc::statfs>::uninit();
    // SAFETY: `stat` points to writable storage and the descriptor remains
    // valid for the duration of `fstatfs`.
    if unsafe { libc::fstatfs(directory.as_raw_fd(), stat.as_mut_ptr()) } != 0 {
        return Err(io_error(
            "verify cgroup2 filesystem",
            io::Error::last_os_error(),
        ));
    }
    // SAFETY: successful `fstatfs` initialized the full structure.
    let stat = unsafe { stat.assume_init() };
    if stat.f_type != CGROUP2_SUPER_MAGIC {
        return Err(unsafe_state(format!(
            "fixed service cgroup is not on cgroup2fs: f_type={:#x}",
            stat.f_type
        )));
    }
    Ok(())
}

#[allow(unsafe_code)]
fn direct_child_directories(directory: &File) -> Result<Vec<String>, CgroupFsError> {
    let dot = CString::new(".").expect("fixed current-directory component has no NUL");
    let flags = libc::O_RDONLY | libc::O_CLOEXEC | libc::O_DIRECTORY | libc::O_NOFOLLOW;
    // SAFETY: `directory` is live and the fixed `.` component is
    // NUL-terminated. Unlike `dup`, `openat` creates a new open file
    // description whose directory offset starts at the beginning for every
    // scan.
    let scan_fd = unsafe { libc::openat(directory.as_raw_fd(), dot.as_ptr(), flags) };
    if scan_fd < 0 {
        return Err(io_error(
            "reopen cgroup directory for scan",
            io::Error::last_os_error(),
        ));
    }
    // SAFETY: `scan_fd` is an owned directory descriptor. `fdopendir`
    // consumes it on success; on failure we close it below.
    let stream = unsafe { libc::fdopendir(scan_fd) };
    if stream.is_null() {
        let source = io::Error::last_os_error();
        // SAFETY: `fdopendir` failed and therefore did not consume `scan_fd`.
        unsafe { libc::close(scan_fd) };
        return Err(io_error("open cgroup directory stream", source));
    }
    let stream_guard = DirectoryStream(stream);
    let mut children = Vec::new();

    loop {
        // SAFETY: Linux exposes thread-local errno through this function.
        unsafe { *libc::__errno_location() = 0 };
        // SAFETY: `stream_guard` owns a live DIR pointer for this loop.
        let entry = unsafe { libc::readdir(stream_guard.0) };
        if entry.is_null() {
            // SAFETY: reading thread-local errno is valid here.
            let errno = unsafe { *libc::__errno_location() };
            if errno != 0 {
                return Err(io_error(
                    "read cgroup directory",
                    io::Error::from_raw_os_error(errno),
                ));
            }
            break;
        }

        // SAFETY: `readdir` returned a live entry whose d_name is NUL-terminated.
        let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };
        let bytes = name.to_bytes();
        if bytes == b"." || bytes == b".." {
            continue;
        }
        let name = std::str::from_utf8(bytes)
            .map_err(|_| unsafe_state("cgroup child name is not UTF-8"))?;
        let c_name = CString::new(bytes).expect("directory entry excludes interior NUL");
        let mut stat = MaybeUninit::<libc::stat>::uninit();
        // SAFETY: the original directory descriptor is live, the entry name
        // is NUL-terminated, and `stat` is writable output storage.
        if unsafe {
            libc::fstatat(
                directory.as_raw_fd(),
                c_name.as_ptr(),
                stat.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } != 0
        {
            return Err(io_error(
                "stat cgroup direct child",
                io::Error::last_os_error(),
            ));
        }
        // SAFETY: successful `fstatat` initialized the full structure.
        let stat = unsafe { stat.assume_init() };
        if stat.st_mode & libc::S_IFMT == libc::S_IFDIR {
            children.push(name.to_string());
        }
    }
    children.sort();
    Ok(children)
}

struct DirectoryStream(*mut libc::DIR);

impl Drop for DirectoryStream {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: this guard is the sole owner of the DIR pointer.
        unsafe { libc::closedir(self.0) };
    }
}

#[allow(unsafe_code)]
fn current_pid() -> Result<u32, CgroupFsError> {
    // SAFETY: `getpid` has no preconditions.
    let pid = unsafe { libc::getpid() };
    u32::try_from(pid).map_err(|_| unsafe_state("current PID is outside u32"))
}

#[allow(unsafe_code)]
fn current_tid() -> Result<u32, CgroupFsError> {
    // SAFETY: `gettid` has no preconditions.
    let tid = unsafe { libc::gettid() };
    u32::try_from(tid).map_err(|_| unsafe_state("current TID is outside u32"))
}

fn io_error(operation: &'static str, source: io::Error) -> CgroupFsError {
    CgroupFsError::Io { operation, source }
}

fn unsafe_state(reason: impl Into<String>) -> CgroupFsError {
    CgroupFsError::UnsafeState(reason.into())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::fs::{self, File};
    use std::time::{SystemTime, UNIX_EPOCH};

    use std::os::unix::fs::PermissionsExt;

    use super::{
        direct_child_directories, parse_required_counter, parse_words,
        require_empty_subtree_control, require_reusable_manager_state, set_directory_mode,
        MANAGER_DIRECTORY_MODE,
    };

    #[test]
    fn populated_counter_requires_one_well_formed_exact_field() {
        assert_eq!(
            parse_required_counter("populated 0\nfrozen 0\n", "populated")
                .expect("one exact counter"),
            0
        );
        for malformed in [
            "frozen 0\n",
            "populated nope\n",
            "populated 0 extra\n",
            "populated 0\npopulated 0\n",
        ] {
            assert!(parse_required_counter(malformed, "populated").is_err());
        }
    }

    #[test]
    fn controller_readback_is_order_independent_but_rejects_duplicates() {
        assert_eq!(
            parse_words("pids cpu memory\n").expect("controller set"),
            BTreeSet::from(["cpu".to_string(), "memory".to_string(), "pids".to_string()])
        );
        assert!(parse_words("cpu memory pids cpu\n").is_err());
    }

    #[test]
    fn repeated_child_scans_restart_from_the_beginning() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("test clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "boole-native-shadow-cgroup-scan-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("test cgroup-shaped tree");
        let directory = File::open(&root).expect("open test directory");

        assert_eq!(
            direct_child_directories(&directory).expect("first child scan"),
            Vec::<String>::new()
        );
        fs::create_dir(root.join("nested")).expect("create child between scans");
        assert_eq!(
            direct_child_directories(&directory).expect("second child scan"),
            ["nested"]
        );

        drop(directory);
        fs::remove_dir_all(root).expect("remove test tree");
    }

    #[test]
    fn descriptor_mode_fix_restores_search_permission_after_restrictive_creation() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("test clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "boole-native-shadow-cgroup-mode-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&root).expect("create test directory");
        fs::set_permissions(&root, fs::Permissions::from_mode(0o600))
            .expect("model restrictive service umask result");
        let directory = File::open(&root).expect("owner can open mode-0600 directory");

        set_directory_mode(&directory, MANAGER_DIRECTORY_MODE)
            .expect("descriptor-based mode restoration");
        assert_eq!(
            fs::metadata(&root)
                .expect("test metadata")
                .permissions()
                .mode()
                & 0o7777,
            0o700
        );

        drop(directory);
        fs::remove_dir(root).expect("remove test directory");
    }

    #[test]
    fn reusable_manager_must_be_unfrozen_and_domain_typed() {
        assert!(require_reusable_manager_state("populated 0\nfrozen 0\n", "domain\n").is_ok());
        assert!(require_reusable_manager_state("populated 0\nfrozen 1\n", "domain\n").is_err());
        assert!(require_reusable_manager_state("populated 0\nfrozen 0\n", "threaded\n").is_err());
        assert!(require_reusable_manager_state("populated 0\n", "domain\n").is_err());
    }

    #[test]
    fn reusable_manager_rejects_residual_subtree_controllers() {
        assert!(require_empty_subtree_control("").is_ok());
        assert!(require_empty_subtree_control("\n").is_ok());
        assert!(require_empty_subtree_control("cpu\n").is_err());
        assert!(require_empty_subtree_control("cpu pids\n").is_err());
    }
}
