//! Static W0 attack/tool stand-ins. Never accepts source, arbitrary argv or a verdict.
use super::*;
use seccompiler::{
    SeccompAction, SeccompCmpArgLen, SeccompCmpOp, SeccompCondition, SeccompFilter, SeccompRule,
    TargetArch,
};
use std::ffi::CString;
fn output(action: Action, allowed: bool, errno: Option<i32>, workers: u32) -> ProbeOutput {
    ProbeOutput {
        action: action.name().into(),
        allowed,
        errno,
        workers,
        tree: None,
        marker: None,
    }
}
fn socket(action: Action) -> (bool, Option<i32>) {
    let domain = if action == Action::NetworkV6 {
        libc::AF_INET6
    } else {
        libc::AF_INET
    };
    let kind = if action == Action::Dns {
        libc::SOCK_DGRAM
    } else {
        libc::SOCK_STREAM
    };
    let fd = unsafe { libc::socket(domain, kind | libc::SOCK_CLOEXEC, 0) };
    if fd >= 0 {
        unsafe { libc::close(fd) };
        (true, None)
    } else {
        (false, std::io::Error::last_os_error().raw_os_error())
    }
}
pub(super) fn positive_control(
    action: Action,
    scratch: &Path,
    input: &Path,
    secret: &Path,
    outside: &Path,
    unpinned: &Path,
) -> Result<bool> {
    match action {
        Action::NetworkV4 | Action::NetworkV6 | Action::Loopback | Action::Dns => {
            Ok(socket(action).0)
        }
        Action::ReadSecret => Ok(fs::read(secret)? == b"fixture-secret"),
        Action::WriteInput | Action::InheritedFd => {
            Ok(OpenOptions::new().write(true).open(input).is_ok())
        }
        Action::WriteOutside => {
            let f = private_file(outside)?;
            f.sync_all()?;
            drop(f);
            fs::remove_file(outside)?;
            Ok(true)
        }
        Action::ScratchExec | Action::UnpinnedExec => {
            let p = if action == Action::ScratchExec {
                scratch.join("payload")
            } else {
                unpinned.into()
            };
            let result = Command::new(p)
                .arg("tool-v1")
                .env_clear()
                .stdin(Stdio::null())
                .output()?;
            Ok(result.status.success()
                && serde_json::from_slice::<ProbeOutput>(&result.stdout).is_ok_and(|o| o.allowed))
        }
        _ => Ok(true),
    }
}
fn denied_io(action: Action, result: std::io::Result<()>) -> ProbeOutput {
    match result {
        Ok(()) => output(action, true, None, 0),
        Err(e) => output(action, false, e.raw_os_error(), 0),
    }
}
fn tool(path: &Path) -> Result<bool> {
    let r = Command::new(path)
        .arg("tool-v1")
        .env_clear()
        .stdin(Stdio::null())
        .output()?;
    Ok(r.status.success()
        && serde_json::from_slice::<ProbeOutput>(&r.stdout)
            .is_ok_and(|o| o.allowed && o.action == "noop"))
}
fn perform(action: Action, paths: &[String]) -> Result<ProbeOutput> {
    ensure!(paths.len() == 7, "W0 payload path arity");
    let scratch = Path::new(&paths[0]);
    let input = Path::new(&paths[1]);
    let secret = Path::new(&paths[2]);
    let outside = Path::new(&paths[3]);
    let comparator = Path::new(&paths[4]);
    let nanoda = Path::new(&paths[5]);
    let unpinned = Path::new(&paths[6]);
    Ok(match action {
        Action::Noop => output(action, true, None, 0),
        Action::NetworkV4 | Action::NetworkV6 | Action::Loopback | Action::Dns => {
            let (ok, e) = socket(action);
            output(action, ok, e, 0)
        }
        Action::ReadSecret => denied_io(action, fs::read(secret).map(|_| ())),
        Action::WriteOutside => denied_io(action, fs::write(outside, b"forbidden")),
        Action::WriteInput => {
            ensure!(
                fs::read(input)? == b"fixture-input",
                "W0 input must be readable before write-denial control"
            );
            denied_io(action, fs::write(input, b"forbidden"))
        }
        Action::InheritedFd => {
            let n = unsafe { libc::fcntl(4, libc::F_GETFD) };
            output(
                action,
                n >= 0,
                if n < 0 {
                    std::io::Error::last_os_error().raw_os_error()
                } else {
                    None
                },
                0,
            )
        }
        Action::ScratchExec | Action::UnpinnedExec => {
            let p = if action == Action::ScratchExec {
                scratch.join("payload")
            } else {
                unpinned.into()
            };
            let mut visible = File::open(&p)?;
            let mut magic = [0; 4];
            visible.read_exact(&mut magic)?;
            ensure!(
                &magic == b"\x7fELF",
                "W0 executable must be visible before denial control"
            );
            match tool(&p) {
                Ok(v) => output(action, v, None, 0),
                Err(e) => output(
                    action,
                    false,
                    e.downcast_ref::<std::io::Error>()
                        .and_then(|v| v.raw_os_error()),
                    0,
                ),
            }
        }
        Action::Comparator => output(action, tool(comparator)?, None, 1),
        Action::Workers => {
            let mut jobs = vec![];
            for _ in 0..4 {
                let path = nanoda.to_owned();
                jobs.push(thread::spawn(move || tool(&path)));
            }
            let mut n = 0;
            for j in jobs {
                if j.join()
                    .map_err(|_| anyhow::anyhow!("W0 worker thread"))??
                {
                    n += 1;
                }
            }
            output(action, n == 4, None, n)
        }
        Action::Tasks => {
            let mut jobs = vec![];
            for _ in 0..256 {
                match thread::Builder::new().spawn(|| thread::sleep(Duration::from_secs(20))) {
                    Ok(h) => jobs.push(h),
                    Err(_) => break,
                }
            }
            for h in jobs {
                let _ = h.join();
            }
            output(action, false, None, 0)
        }
        Action::Memory => {
            let mut slabs = vec![];
            loop {
                let mut b = vec![0u8; 8 * 1024 * 1024];
                for n in (0..b.len()).step_by(4096) {
                    unsafe { std::ptr::write_volatile(b.as_mut_ptr().add(n), 1) };
                }
                slabs.push(b);
                std::hint::black_box(&slabs);
            }
        }
        Action::SlowPrepare => {
            thread::sleep(Duration::from_secs(20));
            output(action, false, None, 0)
        }
        Action::BlockedRead => {
            let (mut r, _w) = pipe()?;
            let mut b = [0; 1];
            r.read_exact(&mut b)?;
            output(action, false, None, 0)
        }
        Action::SlowD4 => {
            let mut f = File::open(scratch.join("chunk.fifo"))?;
            let mut b = [0; 1];
            f.read_exact(&mut b)?;
            output(action, false, None, 0)
        }
        Action::Tree | Action::HoldTree => {
            // SAFETY: this is a fresh, single-threaded fixture payload. PID-1 and
            // cgroup bounds contain every child, including deliberate session escapes.
            let (mut ack_read, ack_write) = pipe()?;
            let child = unsafe { libc::fork() };
            ensure!(child >= 0, "W0 tree fork");
            if child == 0 {
                unsafe { libc::close(ack_read.as_raw_fd()) };
                if unsafe { libc::setsid() } < 0 {
                    unsafe { libc::_exit(125) }
                }
                let grand = unsafe { libc::fork() };
                if grand < 0 {
                    unsafe { libc::_exit(125) }
                }
                if grand == 0 {
                    let until = Instant::now() + Duration::from_secs(1);
                    while unsafe { libc::getppid() } != 1 && Instant::now() < until {
                        unsafe {
                            libc::usleep(1000);
                        }
                    }
                    let facts = [
                        unsafe { libc::getpid() } as u32,
                        unsafe { libc::getsid(0) } as u32,
                        unsafe { libc::getppid() } as u32,
                    ];
                    let mut bytes = [0u8; 12];
                    for (i, n) in facts.iter().enumerate() {
                        bytes[i * 4..i * 4 + 4].copy_from_slice(&n.to_ne_bytes());
                    }
                    if unsafe {
                        libc::write(ack_write.as_raw_fd(), bytes.as_ptr().cast(), bytes.len())
                    } != bytes.len() as isize
                    {
                        unsafe { libc::_exit(125) }
                    }
                    unsafe { libc::close(ack_write.as_raw_fd()) };
                    loop {
                        unsafe { libc::pause() };
                    }
                }
                unsafe { libc::_exit(0) }
            }
            drop(ack_write);
            let mut confirmed = [0u8; 12];
            ack_read.read_exact(&mut confirmed)?;
            let facts: Vec<u32> = confirmed
                .chunks_exact(4)
                .map(|b| u32::from_ne_bytes(b.try_into().expect("four bytes")))
                .collect();
            ensure!(
                facts[0] > 1 && facts[1] == child as u32 && facts[2] == 1,
                "W0 session/adoption observation"
            );
            let mut result = output(action, true, None, 2);
            result.marker = Some("owned-descendants-ready".into());
            result.tree = Some(crate::w0_types::TreeFacts {
                descendant_pid: facts[0],
                session_id: facts[1],
                session_leader: child as u32,
                adopted_by: facts[2],
            });
            println!("{}", serde_json::to_string(&result)?);
            std::io::stdout().flush()?;
            if action == Action::HoldTree {
                loop {
                    unsafe { libc::pause() };
                }
            }
            result
        }
        Action::Capture => {
            let b = [b'x'; 4096];
            loop {
                std::io::stdout().write_all(&b)?;
            }
        }
    })
}
fn seccomp() -> Result<()> {
    let mut rules: BTreeMap<i64, Vec<SeccompRule>> = super::super::network_egress_syscalls()
        .into_iter()
        .chain([
            libc::SYS_unshare,
            libc::SYS_setns,
            libc::SYS_mount,
            libc::SYS_umount2,
            libc::SYS_pivot_root,
            libc::SYS_ptrace,
            libc::SYS_process_vm_readv,
            libc::SYS_process_vm_writev,
            libc::SYS_bpf,
            libc::SYS_perf_event_open,
            libc::SYS_keyctl,
            libc::SYS_add_key,
            libc::SYS_request_key,
            libc::SYS_open_by_handle_at,
            libc::SYS_name_to_handle_at,
            libc::SYS_memfd_create,
            libc::SYS_execveat,
            libc::SYS_seccomp,
        ])
        .map(|n| (n, vec![]))
        .collect();
    let mut clone = vec![];
    for bit in [
        libc::CLONE_NEWUSER,
        libc::CLONE_NEWNET,
        libc::CLONE_NEWNS,
        libc::CLONE_NEWPID,
        libc::CLONE_NEWIPC,
        libc::CLONE_NEWUTS,
        libc::CLONE_NEWCGROUP,
        libc::CLONE_PTRACE,
    ] {
        clone.push(SeccompRule::new(vec![SeccompCondition::new(
            0,
            SeccompCmpArgLen::Qword,
            SeccompCmpOp::MaskedEq(bit as u64),
            bit as u64,
        )?])?);
    }
    rules.insert(libc::SYS_clone, clone);
    let arch = TargetArch::try_from(std::env::consts::ARCH)?;
    let filter = SeccompFilter::new(
        rules,
        SeccompAction::Allow,
        SeccompAction::Errno(libc::EPERM as u32),
        arch,
    )?;
    let program = seccompiler::BpfProgram::try_from(filter)?;
    let fallback = SeccompFilter::new(
        [(libc::SYS_clone3, vec![])].into_iter().collect(),
        SeccompAction::Allow,
        SeccompAction::Errno(libc::ENOSYS as u32),
        arch,
    )?;
    seccompiler::apply_filter(&seccompiler::BpfProgram::try_from(fallback)?)?;
    seccompiler::apply_filter(&program)?;
    Ok(())
}
fn landlock(executables: &[&Path], input: &Path, unpinned: &Path, scratch: &Path) -> Result<()> {
    #[repr(C)]
    struct Ruleset {
        fs: u64,
        net: u64,
    }
    #[repr(C, packed)]
    struct Beneath {
        rights: u64,
        fd: i32,
    }
    let attr = Ruleset {
        fs: (1 << 15) - 1,
        net: 3,
    };
    let fd = unsafe {
        libc::syscall(
            libc::SYS_landlock_create_ruleset,
            &attr,
            std::mem::size_of::<Ruleset>(),
            0,
        )
    };
    ensure!(fd >= 0, "W0 Landlock create");
    let held = unsafe { OwnedFd::from_raw_fd(fd as i32) };
    for (path, rights) in executables.iter().map(|p| (*p, 5u64)).chain([
        (input, 4),
        (unpinned, 4),
        (scratch, ((1 << 15) - 1) ^ 1),
        (Path::new("/proc"), 12),
        (Path::new("/dev/null"), 6),
        (Path::new("/dev/urandom"), 4),
    ]) {
        let p = CString::new(path.as_os_str().as_encoded_bytes())?;
        let n = unsafe {
            libc::open(
                p.as_ptr(),
                libc::O_PATH | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            )
        };
        ensure!(n >= 0, "W0 Landlock path");
        let file = unsafe { OwnedFd::from_raw_fd(n) };
        let rule = Beneath {
            rights,
            fd: file.as_raw_fd(),
        };
        ensure!(
            unsafe { libc::syscall(libc::SYS_landlock_add_rule, held.as_raw_fd(), 1, &rule, 0) }
                == 0,
            "W0 Landlock exact rule"
        );
    }
    ensure!(
        unsafe { libc::syscall(libc::SYS_landlock_restrict_self, held.as_raw_fd(), 0) } == 0,
        "W0 Landlock restrict"
    );
    Ok(())
}
fn capabilities(enforced: bool) -> Result<Capabilities> {
    let text = fs::read_to_string("/proc/self/status")?;
    let value = |key: &str| -> Result<u64> {
        let s = text
            .lines()
            .find_map(|v| v.strip_prefix(key))
            .context("W0 process state")?
            .trim();
        Ok(u64::from_str_radix(s, 16)?)
    };
    let mode = text
        .lines()
        .find_map(|v| v.strip_prefix("Seccomp:"))
        .context("W0 seccomp state")?
        .trim()
        .parse()?;
    Ok(Capabilities {
        namespaces: namespace_ids()?,
        landlock_abi: unsafe {
            libc::syscall(
                libc::SYS_landlock_create_ruleset,
                std::ptr::null::<u8>(),
                0,
                1,
            )
        } as i32,
        landlock_enforced: enforced,
        seccomp: mode,
        apparmor: fs::read_to_string("/proc/self/attr/current")?.trim().into(),
        dumpable: unsafe { libc::prctl(libc::PR_GET_DUMPABLE, 0, 0, 0, 0) },
        caps_zero: ["CapEff:", "CapPrm:", "CapInh:", "CapBnd:", "CapAmb:"]
            .into_iter()
            .all(|k| value(k).is_ok_and(|v| v == 0)),
        pid_one: unsafe { libc::getpid() } == 1,
    })
}
fn init(args: &[String]) -> Result<()> {
    ensure!(
        args.len() == 10 && unsafe { libc::getpid() } == 1,
        "W0 PID-1 entrypoint only"
    );
    let action = Action::parse(&args[1])?;
    let nonce = args[2].clone();
    let paths = &args[3..];
    let expected: BTreeMap<String, String> =
        serde_json::from_str(&std::env::var("BOOLE_W0_PARENT_NS")?)?;
    #[repr(C)]
    struct Header {
        version: u32,
        pid: i32,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Data {
        effective: u32,
        permitted: u32,
        inheritable: u32,
    }
    // SAFETY: this dedicated, fresh PID-1 process drops every capability before
    // forking the payload, and supplies only value arguments to prctl/capset.
    unsafe {
        ensure!(
            libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) == 0,
            "W0 no-new-privileges"
        );
        ensure!(
            libc::prctl(libc::PR_SET_SECUREBITS, 15, 0, 0, 0) == 0,
            "W0 securebits"
        );
        for n in 0..64 {
            let rc = libc::prctl(libc::PR_CAPBSET_DROP, n, 0, 0, 0);
            ensure!(
                rc == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EINVAL),
                "W0 bounding cap drop"
            );
        }
        let h = Header {
            version: 0x20080522,
            pid: 0,
        };
        let d = [Data {
            effective: 0,
            permitted: 0,
            inheritable: 0,
        }; 2];
        ensure!(
            libc::syscall(libc::SYS_capset, &h, d.as_ptr()) == 0,
            "W0 capset"
        );
        ensure!(
            libc::prctl(
                libc::PR_CAP_AMBIENT,
                libc::PR_CAP_AMBIENT_CLEAR_ALL,
                0,
                0,
                0
            ) == 0,
            "W0 ambient cap drop"
        );
        ensure!(
            libc::prctl(libc::PR_SET_DUMPABLE, 1, 0, 0, 0) == 0,
            "W0 private observer fds"
        );
        ensure!(
            libc::syscall(libc::SYS_close_range, 5u32, u32::MAX, 0u32) == 0,
            "W0 init close descriptors"
        );
    }
    seccomp()?;
    let mut writer = unsafe { File::from_raw_fd(3) };
    serde_json::to_writer(
        &mut writer,
        &Event::Prepared {
            nonce: nonce.clone(),
            capabilities: capabilities(false)?,
        },
    )?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    let mut permission = unsafe { File::from_raw_fd(4) };
    let mut byte = [0];
    permission.read_exact(&mut byte)?;
    ensure!(byte == [b'G'], "W0 independent observer gate");
    drop(permission);
    ensure!(
        unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) } == 0,
        "W0 observer dumpable drop"
    );
    let exe = std::env::current_exe()?;
    landlock(
        &[&exe, Path::new(&paths[4]), Path::new(&paths[5])],
        Path::new(&paths[1]),
        Path::new(&paths[6]),
        Path::new(&paths[0]),
    )?;
    ensure!(
        fs::read(&paths[2]).is_err_and(|e| e.raw_os_error() == Some(libc::EACCES)),
        "W0 enforced Landlock secret denial"
    );
    let caps = capabilities(true)?;
    ensure!(
        caps.pid_one
            && caps.caps_zero
            && caps.dumpable == 0
            && caps.landlock_abi >= 4
            && caps.seccomp == 2
            && caps.apparmor == "boole-gate-bwrap (unconfined)"
            && expected.len() == 6
            && expected
                .iter()
                .all(|(k, v)| caps.namespaces.get(k).is_some_and(|n| n != v)),
        "W0 mandatory kernel capability evidence"
    );
    serde_json::to_writer(
        &mut writer,
        &Event::Ready {
            nonce: nonce.clone(),
            capabilities: caps,
        },
    )?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    let child = unsafe { libc::fork() };
    ensure!(child >= 0, "W0 PID-1 fork");
    if child == 0 {
        let result = (|| -> Result<()> {
            ensure!(
                unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL, 0, 0, 0) } == 0,
                "W0 child parent-death"
            );
            ensure!(
                unsafe { libc::fcntl(3, libc::F_SETFD, libc::FD_CLOEXEC) } == 0,
                "W0 private status CLOEXEC"
            );
            let exe = std::env::current_exe()?;
            landlock(
                &[&exe, Path::new(&paths[4]), Path::new(&paths[5])],
                Path::new(&paths[1]),
                Path::new(&paths[6]),
                Path::new(&paths[0]),
            )?;
            let error = Command::new(exe)
                .args(["payload-v1", action.name()])
                .args(paths)
                .env_clear()
                .exec();
            Err(error.into())
        })();
        eprintln!("W0 payload setup unavailable: {result:?}");
        unsafe { libc::_exit(125) }
    }
    let mut reaped = 0;
    let main_status = loop {
        let mut status = 0;
        let pid = unsafe { libc::waitpid(-1, &mut status, 0) };
        if pid < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            bail!("W0 PID-1 main wait")
        }
        if pid == child {
            break status;
        }
        reaped += 1;
    };
    // A PID namespace, not a process group, owns these deliberate setsid/double-
    // fork probes. No host numeric PID is ever signalled here.
    unsafe { libc::kill(-1, libc::SIGKILL) };
    loop {
        let mut status = 0;
        let pid = unsafe { libc::waitpid(-1, &mut status, 0) };
        if pid > 0 {
            reaped += 1;
            continue;
        }
        if pid < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
            continue;
        }
        ensure!(
            pid < 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ECHILD),
            "W0 PID-1 reaping"
        );
        break;
    }
    let code = if libc::WIFEXITED(main_status) {
        libc::WEXITSTATUS(main_status)
    } else {
        128 + libc::WTERMSIG(main_status)
    };
    serde_json::to_writer(
        &mut writer,
        &Event::Finished {
            nonce,
            code,
            reaped,
        },
    )?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}
pub(super) fn helper(args: &[String]) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("init-v1") => init(args),
        Some("payload-v1") => {
            ensure!(args.len() == 9, "W0 payload arity");
            let c = capabilities(true)?;
            ensure!(
                unsafe { libc::geteuid() } == 0
                    && c.caps_zero
                    && c.seccomp == 2
                    && c.apparmor == "boole-gate-bwrap (unconfined)"
                    && unsafe { libc::prctl(libc::PR_GET_NO_NEW_PRIVS, 0, 0, 0, 0) } == 1,
                "W0 payload entry requires the enforced fixture boundary"
            );
            let result = perform(Action::parse(&args[1])?, &args[2..])?;
            println!("{}", serde_json::to_string(&result)?);
            Ok(())
        }
        Some("tool-v1") => {
            ensure!(args.len() == 1, "W0 stand-in tool has no caller argv");
            println!(
                "{}",
                serde_json::to_string(&output(Action::Noop, true, None, 1))?
            );
            Ok(())
        }
        _ => bail!("W0 closed helper entrypoint"),
    }
}
