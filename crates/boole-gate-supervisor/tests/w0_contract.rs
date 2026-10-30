#![cfg(feature = "w0-fixture")]
use boole_gate_supervisor::real_w0::*;
fn grant() -> Grant {
    Grant {
        state: "granted".into(),
        owner: "N".into(),
        step: "W0".into(),
        start_unix: 100,
        end_unix: 2800,
        request_id: "window-01".into(),
        generation: 1,
    }
}
#[test]
fn window_is_bounded_current_owned_and_generation_bound() {
    let g = grant();
    assert_eq!(g.validate(101), Ok(()));
    for n in 0..6 {
        let mut b = g.clone();
        match n {
            0 => b.owner = "E".into(),
            1 => b.state = "requested".into(),
            2 => b.step = "W1".into(),
            3 => b.request_id = "../escape".into(),
            4 => b.request_id = "x".repeat(97),
            _ => b.request_id = String::new(),
        };
        assert!(b.validate(101).is_err());
    }
    let mut b = g.clone();
    b.generation = 0;
    assert!(b.validate(101).is_err());
    b = g.clone();
    b.end_unix += 1;
    assert!(b.validate(101).is_err());
    assert!(g.validate(99).is_err());
    assert!(g.validate(2671).is_err());
}
#[test]
fn every_layer_is_required_without_best_effort() {
    let good = Capabilities {
        namespaces: [true; 6],
        landlock_abi: 4,
        seccomp: true,
        apparmor: true,
        delegated: true,
    };
    assert_eq!(good.validate(), Ok(()));
    for i in 0..6 {
        let mut b = good.clone();
        b.namespaces[i] = false;
        assert!(b.validate().is_err());
    }
    let mut b = good.clone();
    b.landlock_abi = 3;
    assert!(b.validate().is_err());
    b = good.clone();
    b.seccomp = false;
    assert!(b.validate().is_err());
    b = good.clone();
    b.apparmor = false;
    assert!(b.validate().is_err());
    b = good.clone();
    b.delegated = false;
    assert!(b.validate().is_err());
}
#[test]
fn exact_exec_grants_never_cover_scratch_or_a_runtime_directory() {
    let good = Policy {
        executables: vec![
            "/sealed/probe".into(),
            "/sealed/comparator".into(),
            "/sealed/nanoda".into(),
        ],
        readonly: vec!["/lib".into(), "/usr/lib".into(), "/inputs".into()],
        scratch: "/work/job".into(),
    };
    assert_eq!(good.validate(), Ok(()));
    for path in [
        "/",
        "/usr",
        "/lib",
        "/work/job/payload",
        "relative",
        "/sealed/../bin/sh",
    ] {
        let mut b = good.clone();
        b.executables[0] = path.into();
        assert!(b.validate().is_err(), "{path}");
    }
    let mut b = good.clone();
    b.readonly.push("/".into());
    assert!(b.validate().is_err());
    b = good.clone();
    b.executables.push(b.executables[0].clone());
    assert!(b.validate().is_err());
}
#[test]
fn absolute_deadlines_recheck_and_cleanup_keep_nonterminal_causes() {
    let mut l = Lifecycle {
        started: 100,
        wall: 200,
        recheck: 50,
        recheck_started: None,
        first: None,
    };
    assert_eq!(l.observe(299, false, true), None);
    assert_eq!(l.observe(300, false, true), Some(Stop::Transient));
    l.recheck_started = Some(310);
    l.first = None;
    assert_eq!(l.observe(359, false, true), None);
    assert_eq!(l.observe(360, false, true), Some(Stop::Transient));
    l.first = None;
    assert_eq!(l.observe(311, false, false), Some(Stop::Node));
    l.first = Some(Stop::Transient);
    assert_eq!(l.observe(312, false, false), Some(Stop::Transient));
}
#[test]
fn receipt_consumers_require_controls_binding_and_cleanup_for_all_ten_cases() {
    let g = grant();
    for c in [
        "SBX-network",
        "SBX-filesystem",
        "SBX-exec",
        "SBX-capabilities",
        "SBX-tasks",
        "LIFE-preparation",
        "LIFE-deadlines",
        "LIFE-tree",
        "LIFE-parent-kill",
        "LIFE-faults",
    ] {
        let r = Receipt {
            version: 1,
            case: c.into(),
            request_id: g.request_id.clone(),
            generation: g.generation,
            backend_invocations: 1,
            control_pass: true,
            tree_empty: true,
            quiet_window_held: true,
            assertions: required(c)
                .unwrap()
                .iter()
                .map(|v| (v.to_string(), true))
                .collect(),
        };
        let raw = serde_json::to_vec(&r).unwrap();
        assert!(validate_receipt(&raw, c, &g).is_ok(), "{c}");
        for n in 0..6 {
            let mut b = r.clone();
            match n {
                0 => b.control_pass = false,
                1 => b.tree_empty = false,
                2 => b.generation += 1,
                3 => b.backend_invocations = 0,
                4 => b
                    .assertions
                    .insert("unrequested".into(), true)
                    .map(|_| ())
                    .unwrap_or(()),
                _ => {
                    b.assertions.pop_first();
                }
            };
            assert!(validate_receipt(&serde_json::to_vec(&b).unwrap(), c, &g).is_err());
        }
        assert!(validate_receipt(&raw, "unknown", &g).is_err());
    }
}

#[test]
fn evidence_may_not_shadow_a_failed_assertion_with_a_duplicate_key() {
    let g = grant();
    let case = "SBX-network";
    let r = Receipt {
        version: 1,
        case: case.into(),
        request_id: g.request_id.clone(),
        generation: g.generation,
        backend_invocations: 1,
        control_pass: true,
        tree_empty: true,
        quiet_window_held: true,
        assertions: required(case)
            .unwrap()
            .iter()
            .map(|k| (k.to_string(), true))
            .collect(),
    };
    let raw = serde_json::to_string(&r).unwrap();
    let shadowed = raw.replace(
        "\"dns-denied\":true",
        "\"dns-denied\":false,\"dns-denied\":true",
    );
    assert_ne!(raw, shadowed);
    assert!(validate_receipt(shadowed.as_bytes(), case, &g).is_err());
}

#[test]
fn recheck_allowance_cannot_exceed_the_caller_wall_or_thirty_seconds() {
    let mut state = Lifecycle {
        started: 0,
        wall: 10,
        recheck: 30000,
        recheck_started: Some(20),
        first: None,
    };
    assert_eq!(state.observe(30, false, true), Some(Stop::Transient));
    let mut state = Lifecycle {
        started: 0,
        wall: 40000,
        recheck: 40000,
        recheck_started: Some(20),
        first: None,
    };
    assert_eq!(state.observe(30020, false, true), Some(Stop::Transient));
}

#[test]
fn killing_only_the_launcher_is_not_an_observer_death_witness() {
    let g = grant();
    let r = Receipt {
        version: 1,
        case: "LIFE-parent-kill".into(),
        request_id: g.request_id.clone(),
        generation: g.generation,
        backend_invocations: 1,
        control_pass: true,
        tree_empty: true,
        quiet_window_held: true,
        assertions: ["parent-sigkill", "all-descendants-retired", "cgroup-empty"]
            .into_iter()
            .map(|k| (k.to_string(), true))
            .collect(),
    };
    assert!(validate_receipt(&serde_json::to_vec(&r).unwrap(), "LIFE-parent-kill", &g).is_err());
}

#[test]
fn guest_precheck_and_backend_use_identical_bubblewrap_arguments() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let result = std::process::Command::new("python3")
        .args(["-IB"])
        .arg(root.join("scripts/devnet/w0/preflight.py"))
        .arg("flags-v1")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "precheck argument export unavailable"
    );
    let args: Vec<String> = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(args, boole_lean_runner::w0_types::BWRAP_FLAGS);
}
