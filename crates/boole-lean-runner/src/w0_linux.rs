//! Separately versioned Linux W0 fixture backend. Not a proof profile or a
//! production controller capability. The historical byte-adapter API is untouched.
use anyhow::{bail, ensure, Context, Result};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Seek, Write},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::{
            fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
            process::CommandExt,
        },
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
#[path = "w0_linux_payload.rs"]
mod payload;
const CAP: usize = 65536;
const CGROUP_MAGIC: libc::c_long = 0x63677270;
pub use super::w0_types::*;
fn digest(path: &Path) -> Result<String> {
    let mut f = crate::artifact_handles::open_regular(path)?;
    let mut h = Sha256::new();
    let mut b = [0; 65536];
    loop {
        let n = f.read(&mut b)?;
        if n == 0 {
            break;
        }
        h.update(&b[..n]);
    }
    Ok(hex::encode(h.finalize()))
}
fn private_file(path: &Path) -> Result<File> {
    let f = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    ensure!(f.metadata()?.is_file(), "W0 file kind");
    Ok(f)
}
fn owned(path: &Path) -> Result<fs::Metadata> {
    let m = fs::symlink_metadata(path)?;
    ensure!(
        !m.file_type().is_symlink() && m.uid() == unsafe { libc::geteuid() },
        "W0 ownership"
    );
    Ok(m)
}
fn regular_pin(path: &Path, pin: &str, static_only: bool) -> Result<()> {
    ensure!(
        path.is_absolute() && fs::canonicalize(path)? == path,
        "W0 executable canonical path"
    );
    let m = fs::symlink_metadata(path)?;
    ensure!(
        m.is_file() && m.permissions().mode() & 0o7022 == 0 && m.permissions().mode() & 0o111 != 0,
        "W0 executable mode"
    );
    ensure!(digest(path)? == pin, "W0 executable digest");
    if static_only {
        static_elf(path)?;
    }
    Ok(())
}
pub fn static_elf(path: &Path) -> Result<()> {
    let mut f = crate::artifact_handles::open_regular(path)?;
    let mut h = [0; 64];
    f.read_exact(&mut h)?;
    ensure!(
        &h[..7] == b"\x7fELF\x02\x01\x01",
        "W0 static ELF64 required"
    );
    let machine = u16::from_le_bytes(h[18..20].try_into()?);
    ensure!(
        machine
            == if cfg!(target_arch = "aarch64") {
                183
            } else {
                62
            },
        "W0 ELF architecture"
    );
    let offset = u64::from_le_bytes(h[32..40].try_into()?);
    let size = u16::from_le_bytes(h[54..56].try_into()?) as u64;
    let count = u16::from_le_bytes(h[56..58].try_into()?) as u64;
    ensure!(
        size == 56
            && (1..=128).contains(&count)
            && offset
                .checked_add(size * count)
                .is_some_and(|v| v <= f.metadata().map(|m| m.len()).unwrap_or(0)),
        "W0 ELF program table"
    );
    use std::io::{Seek, SeekFrom};
    f.seek(SeekFrom::Start(offset))?;
    for _ in 0..count {
        let mut p = [0; 56];
        f.read_exact(&mut p)?;
        ensure!(
            u32::from_le_bytes(p[..4].try_into()?) != 3,
            "W0 refuses PT_INTERP: direct-loader bypass must not be possible"
        );
    }
    Ok(())
}
fn counter(path: &Path, name: &str) -> Result<u64> {
    let text = fs::read_to_string(path)?;
    text.lines()
        .find_map(|l| {
            let (k, v) = l.split_once(' ')?;
            if k == name {
                v.parse().ok()
            } else {
                None
            }
        })
        .context("W0 counter unavailable")
}
fn namespace_ids() -> Result<BTreeMap<String, String>> {
    let mut v = BTreeMap::new();
    for n in ["user", "net", "pid", "mnt", "ipc", "uts"] {
        v.insert(
            n.into(),
            fs::read_link(format!("/proc/self/ns/{n}"))?
                .to_string_lossy()
                .into_owned(),
        );
    }
    Ok(v)
}
pub fn live(c: &Config) -> Result<()> {
    let read_unix_ns = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let mut inode = None;
    let mut tick = None;
    let mut sequence = None;
    let result = (|| -> Result<()> {
        let mut f = crate::artifact_handles::open_regular(&c.live_grant)?;
        let m = f.metadata()?;
        inode = Some(m.ino());
        ensure!(
            m.is_file() && m.nlink() == 1 && m.len() == crate::w0_types::LIVE_FRAME_SIZE as u64,
            "W0 heartbeat file"
        );
        let end = Instant::now() + Duration::from_millis(200);
        let v = loop {
            let mut raw = vec![];
            f.rewind()?;
            (&mut f)
                .take(crate::w0_types::LIVE_FRAME_SIZE as u64 + 1)
                .read_to_end(&mut raw)?;
            let mut ts = libc::timespec {
                tv_sec: 0,
                tv_nsec: 0,
            };
            ensure!(
                unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) } == 0,
                "W0 monotonic clock"
            );
            let mono = u64::try_from((ts.tv_sec as u128) * 1_000_000_000 + ts.tv_nsec as u128)?;
            match crate::w0_types::validate_live_tick(
                &raw,
                &c.grant_sha256,
                SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
                mono,
            ) {
                Ok(v) => break v,
                Err(e)
                    if e.downcast_ref::<crate::w0_types::FrameTorn>().is_some()
                        && Instant::now() < end =>
                {
                    thread::sleep(Duration::from_millis(2))
                }
                Err(e) => return Err(e),
            }
        };
        tick = Some(v.tick_unix);
        sequence = Some(v.sequence);
        Ok(())
    })();
    let path = c
        .live_grant
        .parent()
        .context("W0 heartbeat parent")?
        .join("live-reads.ndjson");
    let mut log = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    log.lock()?;
    let m = log.metadata()?;
    ensure!(
        m.is_file()
            && m.nlink() == 1
            && m.uid() == unsafe { libc::geteuid() }
            && m.mode() & 0o777 == 0o600
            && m.len() <= 128 * 1024 * 1024,
        "W0 heartbeat evidence identity/bound"
    );
    let error = result
        .as_ref()
        .err()
        .map(|e| e.to_string().chars().take(300).collect::<String>());
    let mut row = serde_json::to_vec(
        &serde_json::json!({"version":3,"source":"guest-stdin-snapshot","reader":"rust","pid":std::process::id(),"read_unix_ns":read_unix_ns,"inode":inode,"tick_unix":tick,"sequence":sequence,"ok":result.is_ok(),"error":error}),
    )?;
    row.push(b'\n');
    log.write_all(&row)?;
    log.sync_all()?;
    result
}

pub fn preflight(c: &Config) -> Result<()> {
    live(c)?;
    ensure!(
        cfg!(any(target_arch = "aarch64", target_arch = "x86_64")),
        "W0 unsupported fixture architecture"
    );
    ensure!(
        c.version == 1 && unsafe { libc::geteuid() } != 0,
        "W0 requires unprivileged delegated user"
    );
    ensure!(
        c.bwrap == Path::new("/usr/bin/bwrap"),
        "W0 fixed AppArmor attachment path"
    );
    let expected = if cfg!(target_arch = "aarch64") {
        "cd6b143283b464baf078ab09ca19dadc5bcc2423b833c4f9dd7e83f9ba1b640b"
    } else {
        "e318903862396f96de3df57264e0158682b952fd3fb53ac23d876413e7b30f71"
    };
    ensure!(c.bwrap_sha256 == expected, "W0 retained bwrap pin");
    regular_pin(&c.bwrap, expected, false)?;
    ensure!(
        fs::canonicalize(std::env::current_exe()?)? == c.probe,
        "W0 controller must be the source-bound private probe"
    );
    for p in [&c.probe, &c.comparator, &c.nanoda] {
        regular_pin(p, &c.probe_sha256, true)?;
    }
    ensure!(
        c.probe != c.comparator && c.probe != c.nanoda && c.comparator != c.nanoda,
        "W0 distinct executable slots"
    );
    ensure!(
        digest(Path::new("/etc/apparmor.d/boole-gate-bwrap"))?
            == "35613442bd82247d132b1102579edc5567d8047930d1b60fbb8bbba93c5abcd6",
        "W0 AppArmor profile pin"
    );
    crate::w0_types::userns_restriction(Path::new(
        "/proc/sys/kernel/apparmor_restrict_unprivileged_userns",
    ))?;
    ensure!(
        unsafe {
            libc::syscall(
                libc::SYS_landlock_create_ruleset,
                std::ptr::null::<u8>(),
                0,
                1,
            )
        } >= 4,
        "W0 Landlock ABI >=4 required"
    );
    ensure!(
        c.cgroup.starts_with("/sys/fs/cgroup")
            && c.cgroup != Path::new("/sys/fs/cgroup")
            && fs::canonicalize(&c.cgroup)? == c.cgroup,
        "W0 delegated cgroup path"
    );
    let dir = File::open(&c.cgroup)?;
    let mut info = std::mem::MaybeUninit::<libc::statfs>::uninit();
    // SAFETY: a live descriptor and correctly sized output buffer.
    ensure!(
        unsafe { libc::fstatfs(dir.as_raw_fd(), info.as_mut_ptr()) } == 0,
        "W0 cgroup filesystem"
    );
    ensure!(
        unsafe { info.assume_init() }.f_type == CGROUP_MAGIC,
        "W0 requires real cgroup-v2 filesystem"
    );
    let own_pidfd = pidfd(std::process::id())?;
    drop(own_pidfd);
    owned(&c.cgroup)?;
    let ctl = fs::read_to_string(c.cgroup.join("cgroup.subtree_control"))?;
    ensure!(
        ["memory", "pids"]
            .iter()
            .all(|v| ctl.split_whitespace().any(|k| k == *v)),
        "W0 memory/pids delegation"
    );
    ensure!(
        fs::read_to_string(c.cgroup.join("cgroup.procs"))?
            .trim()
            .is_empty(),
        "W0 delegate manager must live in its own leaf"
    );
    Ok(())
}
struct Group {
    path: PathBuf,
    handle: File,
    dev: u64,
    ino: u64,
}
impl Group {
    fn create(c: &Config, nonce: &str, pids: u64) -> Result<Self> {
        let path = c.cgroup.join(format!("w0-{nonce}"));
        fs::create_dir(&path)?;
        let handle = crate::artifact_handles::open_regular(&path.join("cgroup.events"))?;
        let m = fs::symlink_metadata(&path)?;
        let g = Self {
            path,
            handle,
            dev: m.dev(),
            ino: m.ino(),
        };
        for (k, v) in [
            ("memory.max", "268435456".to_owned()),
            ("memory.swap.max", "0".to_owned()),
            ("memory.oom.group", "1".to_owned()),
            ("pids.max", pids.to_string()),
        ] {
            fs::write(g.path.join(k), v)?;
        }
        Ok(g)
    }
    fn identity(&self) -> Result<()> {
        let m = fs::symlink_metadata(&self.path)?;
        ensure!(
            m.is_dir()
                && m.dev() == self.dev
                && m.ino() == self.ino
                && self.handle.metadata()?.is_file(),
            "W0 cgroup identity changed"
        );
        Ok(())
    }
    fn empty(&self) -> Result<bool> {
        self.identity()?;
        Ok(counter(&self.path.join("cgroup.events"), "populated")? == 0)
    }
    fn kill(&self) -> Result<()> {
        self.identity()?;
        fs::write(self.path.join("cgroup.kill"), "1")?;
        Ok(())
    }
    fn wait_empty(&self) -> Result<()> {
        let end = Instant::now() + Duration::from_secs(2);
        while !self.empty()? {
            ensure!(Instant::now() < end, "W0 cgroup did not drain");
            thread::sleep(Duration::from_millis(2));
        }
        Ok(())
    }
    fn cleanup(&self) -> Result<()> {
        self.kill()?;
        self.wait_empty()?;
        fs::remove_dir(&self.path)?;
        Ok(())
    }
}
impl Drop for Group {
    fn drop(&mut self) {
        if self.path.exists() {
            let _ = self.cleanup();
        }
    }
}
fn pipe() -> Result<(File, File)> {
    let mut fd = [0; 2];
    ensure!(
        unsafe { libc::pipe2(fd.as_mut_ptr(), libc::O_CLOEXEC) } == 0,
        "W0 pipe"
    );
    Ok(unsafe { (File::from_raw_fd(fd[0]), File::from_raw_fd(fd[1])) })
}
fn duplicate(file: &File) -> Result<OwnedFd> {
    let n = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 10) };
    ensure!(n >= 0, "W0 descriptor duplicate");
    Ok(unsafe { OwnedFd::from_raw_fd(n) })
}
fn drain(
    mut f: File,
    cap: usize,
    overflow: Arc<AtomicBool>,
    ready: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
    mut evidence: File,
) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut out = vec![];
        let mut b = [0; 4096];
        let flags = unsafe { libc::fcntl(f.as_raw_fd(), libc::F_GETFL) };
        if flags < 0
            || unsafe { libc::fcntl(f.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
        {
            overflow.store(true, Ordering::SeqCst);
            return out;
        }
        let mut final_reads = 0;
        loop {
            let n = match f.read(&mut b) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    if cancel.load(Ordering::SeqCst) {
                        break;
                    }
                    thread::sleep(Duration::from_millis(1));
                    continue;
                }
                Err(_) => {
                    overflow.store(true, Ordering::SeqCst);
                    break;
                }
            };
            if out.len() + n > cap {
                overflow.store(true, Ordering::SeqCst);
            }
            let keep = n.min(cap.saturating_sub(out.len()));
            if evidence.write_all(&b[..keep]).is_err() {
                overflow.store(true, Ordering::SeqCst);
                break;
            }
            out.extend_from_slice(&b[..keep]);
            if out
                .windows(b"\"event\":\"prepared\"".len())
                .any(|v| v == b"\"event\":\"prepared\"")
                || out
                    .windows(b"owned-descendants-ready".len())
                    .any(|v| v == b"owned-descendants-ready")
            {
                if evidence.sync_all().is_err() {
                    overflow.store(true, Ordering::SeqCst);
                    break;
                }
                ready.store(true, Ordering::SeqCst);
            }
            if cancel.load(Ordering::SeqCst) {
                final_reads += 1;
                if final_reads > cap / 4096 + 1 {
                    break;
                }
            }
        }
        if evidence.sync_all().is_err() {
            overflow.store(true, Ordering::SeqCst);
        }
        out
    })
}
fn pidfd(pid: u32) -> Result<OwnedFd> {
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
    ensure!(fd >= 0, "W0 pidfd unavailable; no numeric-PID fallback");
    Ok(unsafe { OwnedFd::from_raw_fd(fd as i32) })
}
fn kill_pidfd(fd: &OwnedFd) -> Result<()> {
    ensure!(
        unsafe {
            libc::syscall(
                libc::SYS_pidfd_send_signal,
                fd.as_raw_fd(),
                libc::SIGKILL,
                std::ptr::null::<u8>(),
                0,
            )
        } == 0
            || std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH),
        "W0 pidfd signal"
    );
    Ok(())
}
fn independent_pid_one(
    group: &Path,
    parent: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>> {
    let mut found = None;
    for p in fs::read_to_string(group.join("cgroup.procs"))?.split_whitespace() {
        let pid: u32 = p.parse()?;
        let status = match fs::read_to_string(format!("/proc/{pid}/status")) {
            Ok(v) => v,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e.into()),
        };
        let field = |k: &str| {
            status
                .lines()
                .find_map(|line| line.strip_prefix(k))
                .map(str::trim)
        };
        if field("NSpid:").and_then(|v| v.split_whitespace().last()) != Some("1") {
            continue;
        }
        ensure!(found.is_none(), "W0 ambiguous PID-1 observation");
        ensure!(
            ["CapEff:", "CapPrm:", "CapInh:", "CapBnd:", "CapAmb:"]
                .into_iter()
                .all(|k| field(k).is_some_and(|v| u64::from_str_radix(v, 16) == Ok(0)))
                && field("Seccomp:") == Some("2"),
            "W0 independent capability check"
        );
        let mut ns = BTreeMap::new();
        for k in ["user", "net", "pid", "mnt", "ipc", "uts"] {
            let id = fs::read_link(format!("/proc/{pid}/ns/{k}"))?
                .to_string_lossy()
                .into_owned();
            ensure!(parent.get(k) != Some(&id), "W0 namespace not isolated");
            ns.insert(k.into(), id);
        }
        found = Some(ns);
    }
    found.context("W0 independently observed PID-1 absent")
}
pub fn run(
    c: &Config,
    root: &Path,
    action: Action,
    wall: Duration,
    parent_kill: bool,
) -> Result<Observation> {
    preflight(c)?;
    ensure!(
        (Duration::from_millis(20)..=Duration::from_secs(10)).contains(&wall),
        "W0 fixture wall bound"
    );
    crate::artifact_handles::private_directory(root)?;
    let t = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let nonce = format!("{}-{t}", std::process::id());
    let g = Group::create(c, &nonce, 32)?;
    if action == Action::SlowD4 {
        let base = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
            .open("/dev/shm")?;
        let mut info = std::mem::MaybeUninit::<libc::statfs>::uninit();
        ensure!(
            unsafe { libc::fstatfs(base.as_raw_fd(), info.as_mut_ptr()) } == 0
                && unsafe { info.assume_init() }.f_type == 0x01021994,
            "W0 SlowD4 requires guest-local tmpfs"
        );
    }
    let scratch = crate::w0_types::scratch_path(root, action, &nonce);
    fs::create_dir(&scratch)?;
    fs::set_permissions(&scratch, fs::Permissions::from_mode(0o700))?;
    let input = root.join(format!("input-{nonce}"));
    let mut fi = private_file(&input)?;
    fi.write_all(b"fixture-input")?;
    fi.sync_all()?;
    let secret = root.join(format!("secret-{nonce}"));
    private_file(&secret)?.write_all(b"fixture-secret")?;
    let outside = root.join(format!("outside-{nonce}"));
    let unpinned = root.join(format!("unpinned-{nonce}"));
    fs::copy(&c.probe, &unpinned)?;
    fs::set_permissions(&unpinned, fs::Permissions::from_mode(0o500))?;
    fs::copy(&c.probe, scratch.join("payload"))?;
    fs::set_permissions(scratch.join("payload"), fs::Permissions::from_mode(0o700))?;
    if action == Action::SlowD4 {
        let name =
            std::ffi::CString::new(scratch.join("chunk.fifo").as_os_str().as_encoded_bytes())?;
        ensure!(
            unsafe { libc::mkfifo(name.as_ptr(), 0o600) } == 0,
            "W0 FIFO fixture"
        );
    }
    let host_ns = namespace_ids()?;
    let host_ns_raw = serde_json::to_string(&host_ns)?;
    let (private_read, private_write) = pipe()?;
    let private_source = duplicate(&private_write)?;
    let (permission_read, mut permission_write) = pipe()?;
    let marker = duplicate(&permission_read)?;
    let group_file = OpenOptions::new()
        .write(true)
        .open(g.path.join("cgroup.procs"))?;
    let group_source = duplicate(&group_file)?;
    let mut cmd = Command::new(&c.bwrap);
    cmd.args(crate::w0_types::BWRAP_FLAGS);
    for p in [
        &c.probe,
        &c.comparator,
        &c.nanoda,
        &input,
        &unpinned,
        &secret,
    ] {
        cmd.arg("--ro-bind").arg(p).arg(p);
    }
    cmd.arg("--bind")
        .arg(&scratch)
        .arg(&scratch)
        .arg("--chdir")
        .arg(&scratch)
        .arg("--setenv")
        .arg("BOOLE_W0_PARENT_NS")
        .arg(host_ns_raw);
    cmd.arg(&c.probe)
        .args(["init-v1", action.name(), &nonce])
        .args([
            &scratch,
            &input,
            &secret,
            &outside,
            &c.comparator,
            &c.nanoda,
            &unpinned,
        ]);
    cmd.env_clear()
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let control_pass =
        payload::positive_control(action, &scratch, &input, &secret, &outside, &unpinned)?;
    let began = Instant::now();
    let raw_child = spawn_fixed(&cmd, &group_source, &private_source, &marker)?;
    drop(cmd);
    drop(private_write);
    drop(private_source);
    let mut child = Running {
        inner: raw_child,
        group: &g,
        drains: vec![],
        cancel: Arc::new(AtomicBool::new(false)),
    };
    let overflow = Arc::new(AtomicBool::new(false));
    let ready = Arc::new(AtomicBool::new(false));
    let out = drain(
        child.stdout.take().context("W0 stdout")?,
        CAP,
        overflow.clone(),
        ready.clone(),
        child.cancel.clone(),
        private_file(&root.join(format!("{nonce}.stdout")))?,
    );
    let err = drain(
        child.stderr.take().context("W0 stderr")?,
        CAP,
        overflow.clone(),
        Arc::new(AtomicBool::new(false)),
        child.cancel.clone(),
        private_file(&root.join(format!("{nonce}.stderr")))?,
    );
    let private_overflow = Arc::new(AtomicBool::new(false));
    let prepared = Arc::new(AtomicBool::new(false));
    let status = drain(
        private_read,
        16384,
        private_overflow.clone(),
        prepared.clone(),
        child.cancel.clone(),
        private_file(&root.join(format!("{nonce}.private.ndjson")))?,
    );
    child.drains = vec![Some(out), Some(err), Some(status)];
    let mut physical = None;
    let pidfd_used = fs::read_to_string(format!(
        "/proc/self/fdinfo/{}",
        child.inner.pidfd.as_raw_fd()
    ))?
    .lines()
    .any(|s| s.trim() == format!("Pid:\t{}", child.inner.pid));
    let mut observer_ns = None;
    let mut observer_ready_published = false;
    let mut killed = false;
    loop {
        live(c)?;
        if prepared.load(Ordering::SeqCst) && observer_ns.is_none() {
            observer_ns = Some(independent_pid_one(&g.path, &host_ns)?);
            permission_write.write_all(b"G")?;
        }
        if overflow.load(Ordering::SeqCst) {
            physical = Some(Physical::Capture);
        } else if counter(&g.path.join("memory.events"), "oom_kill")? > 0 {
            physical = Some(Physical::Memory);
        } else if counter(&g.path.join("pids.events"), "max")? > 0 {
            physical = Some(Physical::Tasks);
        } else if began.elapsed() >= wall {
            physical = Some(Physical::Wall);
        }
        if !parent_kill
            && action == Action::HoldTree
            && ready.load(Ordering::SeqCst)
            && !observer_ready_published
        {
            let raw = serde_json::to_vec(&g.path)?;
            let stage = root.join("observer-ready.tmp");
            let mut f = private_file(&stage)?;
            f.write_all(&raw)?;
            f.sync_all()?;
            fs::rename(stage, root.join("observer-ready.observation.json"))?;
            observer_ready_published = true;
        }
        if parent_kill && ready.load(Ordering::SeqCst) {
            kill_pidfd(&child.inner.pidfd)?;
            killed = true;
            break;
        }
        if physical.is_some() {
            g.kill()?;
            break;
        }
        if child.try_wait()?.is_some() {
            break;
        }
        thread::sleep(Duration::from_millis(2));
    }
    let mut automatic_empty = false;
    if killed {
        automatic_empty = g.wait_empty().is_ok();
    }
    g.kill()?;
    let _ = child.wait();
    let empty = g.wait_empty().is_ok();
    child.cancel.store(true, Ordering::SeqCst);
    let output = child.drain(0)?;
    let _diagnostic = child.drain(1)?;
    let private = child.drain(2)?;
    let channel = if private_overflow.load(Ordering::SeqCst) {
        None
    } else {
        parse_channel(&private, &nonce, &host_ns, physical.is_some() || killed).ok()
    };
    let valid = channel
        .as_ref()
        .is_some_and(|v| observer_ns.as_ref() == Some(&v.capabilities.namespaces));
    let (caps, final_code, reaped) = channel
        .map(|v| (Some(v.capabilities), v.code, v.reaped))
        .unwrap_or((None, None, 0));
    // A missing or contradictory private channel is D-node, including a capture
    // interruption. Never let a payload's output manufacture transient evidence.
    if !valid {
        physical = None;
    }
    let pids_events = counter(&g.path.join("pids.events"), "max")?;
    let cleanup_complete = empty && g.cleanup().is_ok();
    let observation = Observation {
        action,
        control_pass,
        physical,
        setup_ok: valid && (final_code == Some(0) || physical.is_some() || killed),
        cleanup_complete,
        private_channel_valid: valid,
        capabilities: caps,
        output,
        reaped,
        pids_events,
        elapsed_ms: began.elapsed().as_millis() as u64,
        pidfd_used,
        observer_cross_checked: valid,
        nonce: nonce.clone(),
        parent_namespaces: host_ns,
        private_channel: private,
        launcher_killed: killed,
        automatic_tree_empty: automatic_empty,
    };
    private_file(&root.join(format!("{nonce}.observation.json")))?
        .write_all(&serde_json::to_vec(&observation)?)?;
    Ok(observation)
}
/// Kill the outer observer, not its bwrap launcher. Its unreaped Child handle
/// prevents PID reuse; inspect the group before any corrective cgroup.kill.
pub fn observer_death(c: &Config, root: &Path) -> Result<crate::w0_types::ObserverDeath> {
    use std::os::unix::process::ExitStatusExt;
    preflight(c)?;
    let config = root.join("observer-config.observation.json");
    private_file(&config)?.write_all(&serde_json::to_vec(c)?)?;
    struct Owned(std::process::Child, PathBuf);
    impl Owned {
        fn cleanup_groups(&self) {
            if let Ok(entries) = fs::read_dir(&self.1) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if entry
                        .file_name()
                        .to_string_lossy()
                        .starts_with(&format!("w0-{}-", self.0.id()))
                        && fs::canonicalize(&path).is_ok_and(|p| p == path)
                    {
                        if let (Ok(m), Ok(handle)) = (
                            fs::symlink_metadata(&path),
                            crate::artifact_handles::open_regular(&path.join("cgroup.events")),
                        ) {
                            if m.is_dir() {
                                let _ = Group {
                                    path,
                                    handle,
                                    dev: m.dev(),
                                    ino: m.ino(),
                                }
                                .cleanup();
                            }
                        }
                    }
                }
            }
        }
    }
    impl Drop for Owned {
        fn drop(&mut self) {
            let _ = self.0.kill();
            self.cleanup_groups();
            let _ = self.0.wait();
        }
    }
    let mut command = Command::new(&c.probe);
    use std::os::unix::process::CommandExt;
    let parent = unsafe { libc::getpid() };
    // SAFETY: only async-signal-safe prctl/getppid before exec, no allocations.
    unsafe {
        command.pre_exec(move || {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL, 0, 0, 0) != 0
                || libc::getppid() != parent
            {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = Owned(
        command
            .arg("observer-v1")
            .arg(&config)
            .arg(root)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .stdin(Stdio::null())
            .stdout(private_file(&root.join("observer.stdout"))?)
            .stderr(private_file(&root.join("observer.stderr"))?)
            .spawn()?,
        c.cgroup.clone(),
    );
    let end = Instant::now() + Duration::from_secs(5);
    let marker = root.join("observer-ready.observation.json");
    while !marker.exists() {
        ensure!(
            child.0.try_wait()?.is_none() && Instant::now() < end,
            "W0 observer readiness absent"
        );
        thread::sleep(Duration::from_millis(2));
    }
    let mut raw = vec![];
    crate::artifact_handles::open_regular(&marker)?
        .take(16385)
        .read_to_end(&mut raw)?;
    ensure!(raw.len() <= 16384, "W0 observer marker bound");
    let path: PathBuf = serde_json::from_slice(&raw)?;
    ensure!(
        path.parent() == Some(c.cgroup.as_path())
            && path.file_name().is_some_and(|n| n
                .to_string_lossy()
                .starts_with(&format!("w0-{}-", child.0.id())))
            && fs::canonicalize(&path)? == path,
        "W0 observer group ownership"
    );
    let m = fs::metadata(&path)?;
    let g = Group {
        handle: crate::artifact_handles::open_regular(&path.join("cgroup.events"))?,
        path,
        dev: m.dev(),
        ino: m.ino(),
    };
    g.identity()?;
    let members_before_kill = fs::read_to_string(g.path.join("cgroup.procs"))?
        .split_whitespace()
        .count();
    child.0.kill()?;
    let status = child.0.wait()?;
    let automatic_empty = g.wait_empty().is_ok();
    let witness = crate::w0_types::ObserverDeath {
        sigkill: status.signal() == Some(libc::SIGKILL),
        members_before_kill,
        automatic_empty,
        cleanup_complete: g.cleanup().is_ok(),
    };
    private_file(&root.join("observer-death.observation.json"))?
        .write_all(&serde_json::to_vec(&witness)?)?;
    Ok(witness)
}
/// Entrypoints used only by the separately built static fixture executable.
pub fn helper(arguments: &[String]) -> Result<()> {
    payload::helper(arguments)
}

struct Running<'a> {
    inner: RawChild,
    group: &'a Group,
    drains: Vec<Option<thread::JoinHandle<Vec<u8>>>>,
    cancel: Arc<AtomicBool>,
}
impl std::ops::Deref for Running<'_> {
    type Target = RawChild;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
impl std::ops::DerefMut for Running<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}
impl Running<'_> {
    fn drain(&mut self, i: usize) -> Result<Vec<u8>> {
        self.drains[i]
            .take()
            .context("W0 drain already consumed")?
            .join()
            .map_err(|_| anyhow::anyhow!("W0 drain failed"))
    }
}
impl Drop for Running<'_> {
    fn drop(&mut self) {
        let _ = self.group.kill();
        let _ = kill_pidfd(&self.inner.pidfd);
        let _ = self.inner.wait();
        let _ = self.group.wait_empty();
        self.cancel.store(true, Ordering::SeqCst);
        for handle in &mut self.drains {
            if let Some(h) = handle.take() {
                let _ = h.join();
            }
        }
    }
}

/// Dedicated fresh-helper spawn, with no std::Command private error-pipe fd
/// that could collide with the explicitly passed status descriptors 3 and 4.
struct RawChild {
    pid: libc::pid_t,
    pidfd: OwnedFd,
    stdout: Option<File>,
    stderr: Option<File>,
    status: Option<i32>,
}
impl RawChild {
    fn try_wait(&mut self) -> Result<Option<i32>> {
        if self.status.is_some() {
            return Ok(self.status);
        }
        let mut status = 0;
        let n = unsafe { libc::waitpid(self.pid, &mut status, libc::WNOHANG) };
        if n == 0 {
            return Ok(None);
        }
        if n < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
            return Ok(None);
        }
        ensure!(n == self.pid, "W0 waitpid identity");
        self.status = Some(status);
        Ok(self.status)
    }
    fn wait(&mut self) -> Result<i32> {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            ensure!(
                Instant::now() < deadline,
                "W0 child reap deadline; publication withheld"
            );
            if let Some(v) = self.try_wait()? {
                return Ok(v);
            }
            thread::sleep(Duration::from_millis(1));
        }
    }
}
impl Drop for RawChild {
    fn drop(&mut self) {
        let _ = kill_pidfd(&self.pidfd);
        let _ = self.wait();
    }
}
fn spawn_fixed(
    cmd: &Command,
    group: &OwnedFd,
    status: &OwnedFd,
    marker: &OwnedFd,
) -> Result<RawChild> {
    use std::ffi::CString;
    let executable = CString::new(cmd.get_program().as_encoded_bytes())?;
    let mut arguments = vec![executable.clone()];
    for a in cmd.get_args() {
        arguments.push(CString::new(a.as_encoded_bytes())?);
    }
    let mut pointers: Vec<*const libc::c_char> = arguments.iter().map(|v| v.as_ptr()).collect();
    pointers.push(std::ptr::null());
    let environment = [
        CString::new("PATH=/usr/bin:/bin")?,
        CString::new("LC_ALL=C")?,
    ];
    let env = [
        environment[0].as_ptr(),
        environment[1].as_ptr(),
        std::ptr::null(),
    ];
    let (out_read, out_write) = pipe()?;
    let (err_read, err_write) = pipe()?;
    let out = duplicate(&out_write)?;
    let err = duplicate(&err_write)?;
    let null = File::open("/dev/null")?;
    let input = duplicate(&null)?;
    let parent = unsafe { libc::getpid() };
    // SAFETY: all strings/fds/buffers are prepared before fork. The child uses
    // only libc async-signal-safe primitives, then execve or _exit; it neither
    // allocates nor runs Rust destructors. The parent owns/reaps this exact PID.
    #[repr(C)]
    struct CloneArgs {
        flags: u64,
        pidfd: u64,
        child_tid: u64,
        parent_tid: u64,
        exit_signal: u64,
        stack: u64,
        stack_size: u64,
        tls: u64,
        set_tid: u64,
        set_tid_size: u64,
        cgroup: u64,
    }
    let mut child_handle: i32 = -1;
    let args = CloneArgs {
        flags: libc::CLONE_PIDFD as u64,
        pidfd: (&mut child_handle as *mut i32) as u64,
        child_tid: 0,
        parent_tid: 0,
        exit_signal: libc::SIGCHLD as u64,
        stack: 0,
        stack_size: 0,
        tls: 0,
        set_tid: 0,
        set_tid_size: 0,
        cgroup: 0,
    };
    // CLONE_PIDFD allocates the stable handle atomically with the child; fd
    // pressure or a missing syscall refuses before any child exists.
    let pid = unsafe { libc::syscall(libc::SYS_clone3, &args, std::mem::size_of::<CloneArgs>()) }
        as libc::pid_t;
    ensure!(pid >= 0, "W0 clone3/pidfd required; no fallback");
    if pid == 0 {
        unsafe {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL, 0, 0, 0) != 0
                || libc::getppid() != parent
            {
                libc::_exit(125)
            }
            let mut digits = [0u8; 32];
            let mut n = libc::getpid() as u32;
            let mut pos = digits.len();
            loop {
                pos -= 1;
                digits[pos] = b'0' + (n % 10) as u8;
                n /= 10;
                if n == 0 {
                    break;
                }
            }
            if libc::write(
                group.as_raw_fd(),
                digits[pos..].as_ptr().cast(),
                digits.len() - pos,
            ) != (digits.len() - pos) as isize
            {
                libc::_exit(125)
            }
            for (source, target) in [
                (input.as_raw_fd(), 0),
                (out.as_raw_fd(), 1),
                (err.as_raw_fd(), 2),
                (status.as_raw_fd(), 3),
                (marker.as_raw_fd(), 4),
            ] {
                if libc::dup2(source, target) < 0 {
                    libc::_exit(125)
                }
            }
            if libc::syscall(libc::SYS_close_range, 5u32, u32::MAX, 0u32) < 0 {
                libc::_exit(125)
            }
            libc::execve(executable.as_ptr(), pointers.as_ptr(), env.as_ptr());
            libc::_exit(125)
        }
    }
    ensure!(child_handle >= 0, "W0 atomic child pidfd");
    let handle = unsafe { OwnedFd::from_raw_fd(child_handle) };
    Ok(RawChild {
        pid,
        pidfd: handle,
        stdout: Some(out_read),
        stderr: Some(err_read),
        status: None,
    })
}
