#![cfg(target_os = "macos")]
use boole_gate_supervisor::{process::Envelope, real_process::run};
use std::{process::Command, time::Instant};
#[test]
fn mac_controller_bounds_deadline_and_both_output_channels() {
    let e = Envelope {
        memory_mib: 512,
        wall_ms: 50,
        output_cap: 1024,
    };
    let start = Instant::now();
    let out = run(
        Command::new("/usr/bin/python3").args(["-B", "-c", "import time; time.sleep(0.6)"]),
        &e,
    );
    assert!(
        out.is_err() && start.elapsed().as_millis() < 500,
        "deadline ignored"
    );
    for channel in ["stdout", "stderr"] {
        let code = format!("import sys;sys.{channel}.write('x'*2000)");
        assert!(
            run(
                Command::new("/usr/bin/python3").args(["-B", "-c", &code]),
                &e
            )
            .is_err(),
            "capture cap ignored on {channel}"
        );
    }
}
#[test]
fn mac_controller_reaps_a_trusted_child_that_started_its_own_session() -> anyhow::Result<()> {
    use boole_gate_supervisor::codec::Scratch;
    let root = Scratch::new(&std::env::temp_dir())?;
    let pidfile = root.0.join("pid");
    let code="import subprocess,sys,time; p=subprocess.Popen(['/usr/bin/python3','-B','-c','import time;time.sleep(5)'],start_new_session=True);open(sys.argv[1],'w').write(str(p.pid));time.sleep(5)";
    let result = run(
        Command::new("/usr/bin/python3")
            .args(["-B", "-c", code])
            .arg(&pidfile),
        &Envelope {
            memory_mib: 512,
            wall_ms: 500,
            output_cap: 1024,
        },
    );
    assert!(result.is_err());
    let pid = std::fs::read_to_string(pidfile)?;
    let status = Command::new("/bin/ps")
        .args(["-p", pid.trim(), "-o", "stat="])
        .output()?;
    assert!(
        status.stdout.is_empty()
            || String::from_utf8_lossy(&status.stdout)
                .trim()
                .starts_with('Z'),
        "owned child survived"
    );
    Ok(())
}
