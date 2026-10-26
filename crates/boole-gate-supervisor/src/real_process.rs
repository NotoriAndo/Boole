//! Bounded Mac development controller, separate from the stub byte sandbox.
//! Trusted adapter/tools may spawn; this is not the future Linux controller.
use crate::process::Envelope;
use anyhow::{ensure, Context, Result};
use std::{
    collections::BTreeMap,
    io::Read,
    process::{Child, Command, Output, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread,
    time::{Duration, Instant},
};
#[derive(Debug)]
pub struct Unavailable(pub &'static str);
impl std::fmt::Display for Unavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Mac controller {}", self.0)
    }
}
impl std::error::Error for Unavailable {}
#[derive(Clone)]
struct Row {
    pid: u32,
    parent: u32,
    group: u32,
    rss: u64,
    zombie: bool,
    birth: String,
}
fn rows() -> Result<Vec<Row>> {
    let out = Command::new("/bin/ps")
        .args(["-axo", "pid=,ppid=,pgid=,rss=,stat=,lstart="])
        .output()?;
    ensure!(out.status.success(), "controller process inventory");
    let mut result = vec![];
    for line in std::str::from_utf8(&out.stdout)?.lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() < 10 {
            continue;
        }
        result.push(Row {
            pid: fields[0].parse()?,
            parent: fields[1].parse()?,
            group: fields[2].parse()?,
            rss: fields[3].parse::<u64>()?.saturating_mul(1024),
            zombie: fields[4].starts_with('Z'),
            birth: fields[5..].join(" "),
        });
    }
    Ok(result)
}
fn signal(pid: u32, birth: &str, name: &str) {
    if let Ok(out) = Command::new("/bin/ps")
        .args(["-p", &pid.to_string(), "-o", "lstart="])
        .output()
    {
        if String::from_utf8_lossy(&out.stdout)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            == birth
        {
            let _ = Command::new("/bin/kill")
                .args([name, &pid.to_string()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }
}
struct Owned {
    child: Child,
    known: BTreeMap<u32, String>,
    clean: bool,
    #[cfg(test)]
    fail_inventory: bool,
}
impl Owned {
    fn sample(&mut self) -> Result<Vec<Row>> {
        #[cfg(test)]
        ensure!(!self.fail_inventory, "fixture ps unavailable");
        let rows = rows()?;
        let leader = self.child.id();
        // Expand descendants while their original ancestry is visible; retain births
        // after reparenting and across trusted tools' own process-group changes.
        let mut selected: BTreeMap<u32, String> = rows
            .iter()
            .filter(|r| self.known.get(&r.pid) == Some(&r.birth))
            .map(|r| (r.pid, r.birth.clone()))
            .collect();
        if self.known.is_empty() {
            if let Some(r) = rows.iter().find(|r| r.pid == leader) {
                selected.insert(r.pid, r.birth.clone());
            }
        }
        for _ in 0..32 {
            let before = selected.len();
            for r in &rows {
                if selected.contains_key(&r.parent) {
                    selected.insert(r.pid, r.birth.clone());
                }
            }
            if before == selected.len() {
                break;
            }
        }
        // The original live group cannot be reused while one of its known members
        // remains. Include same-group descendants without trusting a reused PID.
        if rows
            .iter()
            .any(|r| r.group == leader && selected.contains_key(&r.pid))
        {
            for r in &rows {
                if r.group == leader {
                    selected.insert(r.pid, r.birth.clone());
                }
            }
        }
        self.known.extend(selected.clone());
        Ok(rows
            .into_iter()
            .filter(|r| !r.zombie && selected.get(&r.pid) == Some(&r.birth))
            .collect())
    }
    fn cleanup(&mut self) -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let active = self.sample()?;
            for r in &active {
                signal(r.pid, &r.birth, "-STOP");
            }
            let active = self.sample()?;
            for r in active.iter().rev() {
                signal(r.pid, &r.birth, "-KILL");
            }
            let _ = self.child.try_wait();
            if self.sample()?.is_empty() {
                let _ = self.child.wait();
                self.clean = true;
                return Ok(());
            }
            ensure!(
                Instant::now() < deadline,
                "controller descendants survived cleanup"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }
}
impl Drop for Owned {
    fn drop(&mut self) {
        if !self.clean {
            if self.cleanup().is_err() {
                // This leader was spawned with process_group(0). Kill its owned
                // group before reaping the leader if inventory is unavailable.
                let _ = Command::new("/bin/kill")
                    .args(["-KILL", "--", &format!("-{}", self.child.id())])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}
fn capture<R: Read + Send + 'static>(
    mut stream: R,
    cap: usize,
    overflow: Arc<AtomicBool>,
) -> mpsc::Receiver<Result<Vec<u8>>> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let result = (|| {
            let mut out = vec![];
            let mut b = [0; 8192];
            loop {
                let n = stream.read(&mut b)?;
                if n == 0 {
                    return Ok(out);
                }
                if n > cap.saturating_sub(out.len()) {
                    overflow.store(true, Ordering::SeqCst);
                    return Err(Unavailable("capture-limit").into());
                }
                out.extend(&b[..n]);
            }
        })();
        let _ = tx.send(result);
    });
    rx
}
pub fn run(command: &mut Command, envelope: &Envelope) -> Result<Output> {
    ensure!(
        cfg!(target_os = "macos"),
        "real controller is Mac development only"
    );
    ensure!(
        envelope.wall_ms > 0 && envelope.memory_mib > 0 && envelope.output_cap >= 128,
        "controller envelope"
    );
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let deadline = Instant::now() + Duration::from_millis(envelope.wall_ms);
    let child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("Mac adapter spawn")?;
    let mut owned = Owned {
        child,
        known: BTreeMap::new(),
        clean: false,
        #[cfg(test)]
        fail_inventory: false,
    };
    let overflow = Arc::new(AtomicBool::new(false));
    let stdout = capture(
        owned.child.stdout.take().context("controller stdout")?,
        envelope.output_cap,
        overflow.clone(),
    );
    let stderr = capture(
        owned.child.stderr.take().context("controller stderr")?,
        envelope.output_cap,
        overflow.clone(),
    );
    let status = loop {
        let memory = owned.sample()?.iter().try_fold(0u64, |sum, r| {
            sum.checked_add(r.rss).context("controller RSS overflow")
        })?;
        if overflow.load(Ordering::SeqCst) {
            return Err(Unavailable("capture-limit").into());
        }
        if memory > envelope.memory_mib.saturating_mul(1024 * 1024) {
            return Err(Unavailable("memory").into());
        }
        if Instant::now() >= deadline {
            return Err(Unavailable("deadline").into());
        }
        if let Some(status) = owned.child.try_wait()? {
            break status;
        }
        thread::sleep(Duration::from_millis(20));
    };
    owned.cleanup()?;
    let stdout = stdout
        .recv_timeout(Duration::from_secs(1))
        .context("controller stdout drain")??;
    let stderr = stderr
        .recv_timeout(Duration::from_secs(1))
        .context("controller stderr drain")??;
    ensure!(
        !overflow.load(Ordering::SeqCst),
        "controller capture overflow"
    );
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

#[cfg(all(test, unix))]
mod fallback_tests {
    use super::*;
    use std::os::unix::process::CommandExt;
    #[test]
    fn failed_inventory_still_kills_the_owned_process_group() -> Result<()> {
        let root = crate::codec::Scratch::new(&std::env::temp_dir())?;
        let pidfile = root.0.join("child");
        let child = Command::new("/bin/sh")
            .args(["-c", "sleep 60 & echo $! > \"$1\"; wait", "fixture"])
            .arg(&pidfile)
            .process_group(0)
            .spawn()?;
        let group = child.id();
        let owned = Owned {
            child,
            known: BTreeMap::new(),
            clean: false,
            fail_inventory: true,
        };
        let end = Instant::now() + Duration::from_secs(2);
        while !pidfile.exists() && Instant::now() < end {
            thread::sleep(Duration::from_millis(10));
        }
        let pid: u32 = std::fs::read_to_string(pidfile)?.trim().parse()?;
        drop(owned);
        thread::sleep(Duration::from_millis(100));
        let alive = rows()?.iter().any(|r| r.pid == pid && !r.zombie);
        // Retire the exact fixture group even on RED, before asserting.
        let _ = Command::new("/bin/kill")
            .args(["-KILL", "--", &format!("-{group}")])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        assert!(
            !alive,
            "inventory failure left the controller descendant alive"
        );
        Ok(())
    }
}
