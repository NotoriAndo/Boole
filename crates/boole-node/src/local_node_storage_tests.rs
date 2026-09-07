use super::*;
use axum::body::Body;

fn config(dir: &Path) -> LocalNodeConfig {
    LocalNodeConfig {
        scenario_path: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/protocol/runtime-smoke/v1.json"),
        block_path: dir.join("blocks.ndjson"),
        reward_ledger_path: None,
        work_manifests_path: None,
        bounties_path: None,
        bounty_event_ledger_path: None,
        bounty_verifiers: None,
        family_manifests_dir: None,
        operator_signer_pks: vec![],
        session_registry_path: None,
        submit_nonce_ledger_path: None,
        signed_nonce_ledger_path: None,
        proof_dedup_ledger_path: None,
        submit_receipt_ledger_path: None,
        receipt_commitment_ledger_path: None,
        max_requests: None,
        genesis_override: None,
        state_dir: None,
        network_id: None,
        lean_checker_dir: None,
        lean_checker_disabled: true,
        http_rate_limit_per_60s: None,
        allow_anonymous_submit: true,
    }
}

#[test]
fn boot_rejects_role_collisions_before_recovering_canonical_bytes() {
    for role in 0..8 {
        let dir = crate::durability::PrivateTempDir::new("boole-storage-role-collision")
            .expect("private fixture");
        let mut cfg = config(dir.path());
        crate::runtime_smoke::run_runtime_smoke_scenario_file(
            cfg.scenario_path.clone(),
            cfg.block_path.clone(),
        )
        .expect("produce canonical fixture");
        let before = std::fs::read(&cfg.block_path).expect("canonical bytes");
        let path = cfg.block_path.clone();
        let target = match role {
            0 => &mut cfg.reward_ledger_path,
            1 => &mut cfg.bounty_event_ledger_path,
            2 => &mut cfg.session_registry_path,
            3 => &mut cfg.submit_nonce_ledger_path,
            4 => &mut cfg.signed_nonce_ledger_path,
            5 => &mut cfg.proof_dedup_ledger_path,
            6 => &mut cfg.submit_receipt_ledger_path,
            _ => &mut cfg.receipt_commitment_ledger_path,
        };
        *target = Some(path.clone());
        let result = LocalNodeState::from_config(cfg);
        assert_eq!(
            std::fs::read(path).expect("preserved canonical"),
            before,
            "role {role} must not rewrite canonical data during rejected boot"
        );
        assert!(
            matches!(result, Err(ref err) if err.to_string().contains("storage role collision")),
            "role {role} must be rejected before store recovery"
        );
    }
}

#[test]
fn boot_reserves_checkpoint_and_lock_metadata_paths_before_recovery() {
    for case in ["checkpoint", "block-lock", "state-lock", "state-manifest"] {
        let dir = crate::durability::PrivateTempDir::new("boole-storage-derived-collision")
            .expect("fixture");
        let mut cfg = config(dir.path());
        crate::runtime_smoke::run_runtime_smoke_scenario_file(
            cfg.scenario_path.clone(),
            cfg.block_path.clone(),
        )
        .expect("canonical fixture");
        let canonical = std::fs::read(&cfg.block_path).expect("canonical bytes");
        let conflict = match case {
            "checkpoint" => crate::checkpoint::checkpoint_path_for(&cfg.block_path),
            "block-lock" => dir.path().join(".blocks.ndjson.boole-lock"),
            "state-lock" => dir.path().join(state_dir::STATE_LOCK_FILE),
            "state-manifest" => dir.path().join(state_dir::STATE_MANIFEST_FILE),
            _ => cfg.block_path.clone(),
        };
        std::fs::write(&conflict, b"existing role bytes, including torn tail")
            .expect("existing metadata");
        cfg.reward_ledger_path = Some(conflict.clone());
        if case.starts_with("state-") {
            cfg.state_dir = Some(dir.path().to_path_buf());
        }
        let before = std::fs::read(&conflict).expect("conflicting bytes");
        let block = cfg.block_path.clone();
        let result = LocalNodeState::from_config(cfg);
        assert!(
            matches!(result, Err(ref error) if error.to_string().contains("storage role collision")),
            "{case}"
        );
        assert_eq!(
            std::fs::read(block).expect("preserved canonical"),
            canonical,
            "{case}"
        );
        assert_eq!(
            std::fs::read(conflict).expect("preserved role"),
            before,
            "{case}"
        );
    }
}

#[test]
fn boot_rejects_mutable_role_aliasing_read_only_inputs_without_overwriting_them() {
    for case in ["scenario", "bounties", "work-manifests", "family-manifest"] {
        let dir = crate::durability::PrivateTempDir::new("boole-storage-input-collision")
            .expect("fixture");
        let mut cfg = config(dir.path());
        let input = dir.path().join("input.json");
        let source = if case == "scenario" {
            std::fs::read(&cfg.scenario_path).expect("scenario fixture")
        } else {
            b"{\"version\":1,\"work\":[],\"bounties\":[]}\n".to_vec()
        };
        std::fs::write(&input, &source).expect("input fixture");
        match case {
            "scenario" => cfg.scenario_path = input.clone(),
            "bounties" => cfg.bounties_path = Some(input.clone()),
            "work-manifests" => cfg.work_manifests_path = Some(input.clone()),
            _ => cfg.family_manifests_dir = Some(dir.path().to_path_buf()),
        }
        cfg.reward_ledger_path = Some(input.clone());
        let result = LocalNodeState::from_config(cfg);
        assert_eq!(
            std::fs::read(&input).expect("read input after boot"),
            source,
            "{case} preserved"
        );
        assert!(
            matches!(result, Err(ref error) if error.to_string().contains("storage role collision")),
            "{case}"
        );
    }
}

#[cfg(unix)]
#[test]
fn boot_resolves_parent_aliases_but_accepts_distinct_roles() {
    let dir =
        crate::durability::PrivateTempDir::new("boole-storage-parent-alias").expect("fixture");
    let real = dir.path().join("real");
    std::fs::create_dir(&real).expect("real parent");
    std::os::unix::fs::symlink(&real, dir.path().join("alias")).expect("parent alias");
    let mut cfg = config(&real);
    crate::runtime_smoke::run_runtime_smoke_scenario_file(
        cfg.scenario_path.clone(),
        cfg.block_path.clone(),
    )
    .expect("canonical fixture");
    let before = std::fs::read(&cfg.block_path).expect("canonical bytes");
    cfg.reward_ledger_path = Some(dir.path().join("alias/blocks.ndjson"));
    assert!(LocalNodeState::from_config(cfg).is_err());
    let mut cfg = config(&real);
    cfg.reward_ledger_path = Some(dir.path().join("alias/rewards.ndjson"));
    let block = cfg.block_path.clone();
    let node = LocalNodeState::from_config(cfg).expect("distinct roles boot normally");
    assert_eq!(node.runtime.cached_blocks().len(), 2);
    assert_eq!(std::fs::read(block).expect("preserved canonical"), before);
    drop(node);
    let mut cfg = config(&real);
    let scenario_alias = dir.path().join("scenario-alias.json");
    std::os::unix::fs::symlink(
        cfg.scenario_path.canonicalize().expect("scenario"),
        &scenario_alias,
    )
    .expect("read-only input symlink");
    cfg.scenario_path = scenario_alias;
    let catalog = dir.path().join("shared-input.json");
    std::fs::write(&catalog, b"{\"version\":1,\"work\":[],\"bounties\":[]}\n")
        .expect("shared catalog");
    cfg.work_manifests_path = Some(catalog.clone());
    let catalog_alias = dir.path().join("catalog-alias.json");
    std::os::unix::fs::symlink(&catalog, &catalog_alias).expect("catalog alias");
    cfg.bounties_path = Some(catalog_alias);
    LocalNodeState::from_config(cfg)
        .expect("read-only aliases and symlinks retain their read policy");
}

#[cfg(unix)]
#[test]
fn signed_http_append_failure_rolls_back_before_next_request_and_restart() {
    const CHILD: &str = "BOOLE_STORAGE_APPEND_TEST_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let result = std::process::Command::new(std::env::current_exe().expect("test binary"))
            .args(["--exact", "local_node::storage_tests::signed_http_append_failure_rolls_back_before_next_request_and_restart", "--nocapture"])
            .env(CHILD, "1")
            .output().expect("isolated fault process");
        assert!(
            result.status.success(),
            "child regression failed:\n{}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        return;
    }
    tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime")
        .block_on(async {
            use tower::ServiceExt;
            let dir = crate::durability::PrivateTempDir::new("boole-http-append-failure").expect("fixture");
            let make_config = || {
                let mut cfg = config(dir.path());
                cfg.signed_nonce_ledger_path = Some(dir.path().join("nonces.ndjson"));
                cfg.receipt_commitment_ledger_path = Some(dir.path().join("receipts.ndjson"));
                cfg
            };
            let key = boole_core::SigningKeyV2::from_dev_id("storage-append-http");
            let receipt = boole_core::ReceiptCommitment::new(boole_core::ReceiptCommitmentInput {
                agent_pk: "aa".repeat(32), family_id: "v1-lenbound".to_string(),
                verifier_id: "lean-runner-v01".to_string(), verifier_hash_version: "v0".to_string(),
                artifact_hash: "22".repeat(32), request_hash: "33".repeat(32),
                result: "accepted".to_string(), fee_charged: "1".to_string(),
                reward_recipient: "44".repeat(32),
            }).expect("receipt fixture");
            let request = |nonce: &str| {
                let payload = json!({"schema": "boole.receipts.commit.v1", "receiptCommitment": receipt,
                    "validBefore": now_unix_ms() / 1000 + 60, "nonce": nonce});
                let signed = key.sign_for_network(&payload, None).expect("signed fixture");
                let body = json!({"schema": signed.schema, "payload": signed.payload, "pk": signed.pk, "signature": signed.signature});
                Request::builder().method("POST").uri("/receipts").header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).expect("serialize"))).expect("request")
            };
            let node = LocalNodeState::from_config(make_config()).expect("boot");
            let state = Arc::new(RwLock::new(node));
            let router = build_router(AppState { inner: state.clone(), rate_limiter: None });
            let response = router.clone().oneshot(request("stable")).await.expect("seed response");
            assert_eq!(response.status(), StatusCode::OK);
            let path = dir.path().join("nonces.ndjson");
            let before = std::fs::read(&path).expect("confirmed nonce bytes");
            let limit = FileSizeLimit::set(before.len() as u64 + 24);
            let response = router.clone().oneshot(request("partial")).await.expect("failed response");
            drop(limit);
            assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
            assert_eq!(std::fs::read(&path).expect("post-failure bytes"), before,
                "failed append must durably restore the confirmed prefix");
            let response = router.clone().oneshot(request("next")).await.expect("next response");
            assert_eq!(response.status(), StatusCode::OK);
            drop(router);
            drop(state);
            let rebooted = LocalNodeState::from_config(make_config()).expect("restart recovers successful requests");
            assert!(!rebooted.signed_nonce_ledger.as_ref().expect("nonces").contains(&key.pk_hex(), "partial"));
            let router = build_router(AppState { inner: Arc::new(RwLock::new(rebooted)), rate_limiter: None });
            for nonce in ["stable", "next"] {
                let response = router.clone().oneshot(request(nonce)).await.expect("replay response");
                assert_eq!(response.status(), StatusCode::CONFLICT, "successful nonce survives restart");
            }
        });
}

#[cfg(unix)]
struct FileSizeLimit {
    prior: libc::rlimit,
    handler: libc::sighandler_t,
}

#[test]
fn indeterminate_nonce_write_blocks_later_mutation_and_preserves_recovery() {
    let dir = crate::durability::PrivateTempDir::new("boole-indeterminate-nonce").expect("fixture");
    let mut cfg = config(dir.path());
    let path = dir.path().join("nonces.ndjson");
    cfg.signed_nonce_ledger_path = Some(path.clone());
    let mut node = LocalNodeState::from_config(cfg).expect("boot");
    burn_signed_envelope_nonce(&mut node, "signer", "stable").expect("confirmed burn");
    crate::durability::fail_next_append(crate::durability::AppendFault {
        fail_sync: true,
        fail_rollback: true,
        ..Default::default()
    });
    assert!(burn_signed_envelope_nonce(&mut node, "signer", "uncertain").is_err());
    let before_retry = std::fs::read(&path).expect("uncertain journal");
    assert!(
        burn_signed_envelope_nonce(&mut node, "signer", "next").is_err(),
        "an indeterminate append must fence every later request until restart"
    );
    assert_eq!(std::fs::read(&path).expect("after retry"), before_retry);
    assert!(!node
        .signed_nonce_ledger
        .as_ref()
        .expect("nonces")
        .contains("signer", "next"));
    let recovered =
        FileSignedNonceLedger::recover(&path).expect("complete uncertain event is recoverable");
    assert!(recovered.contains("signer", "stable"));
    assert!(recovered.contains("signer", "uncertain"));
    assert!(!recovered.contains("signer", "next"));
}

#[tokio::test]
async fn uncertain_session_revoke_fences_only_its_node_until_restart() {
    use tower::ServiceExt;

    const RESTART: &str = "BOOLE_STORAGE_REVOKE_RESTART_FIXTURE";
    let owner = boole_core::SigningKeyV2::from_dev_id("storage-fence-owner");
    let session_key = boole_core::SigningKeyV2::from_dev_id("storage-fence-session");
    let session_pk = session_key.pk_hex();
    let reward = "ff".repeat(32);
    let make_config = |dir: &Path| {
        let mut cfg = config(dir);
        cfg.session_registry_path = Some(dir.join("sessions.ndjson"));
        cfg.submit_nonce_ledger_path = Some(dir.join("submit-nonces.ndjson"));
        cfg.reward_ledger_path = Some(dir.join("rewards.ndjson"));
        cfg
    };
    let signed = |payload: Value, key: &boole_core::SigningKeyV2| {
        let signed = key.sign_for_network(&payload, None).expect("sign request");
        json!({"schema": signed.schema, "payload": signed.payload, "pk": signed.pk,
            "signature": signed.signature})
    };
    let post = |path: &str, body: &Value| {
        Request::builder()
            .method("POST")
            .uri(path)
            .header("content-type", "application/json")
            .extension(ConnectInfo(HttpRemoteAddr {
                socket_addr: "127.0.0.1:31001".parse().expect("remote address"),
                header_received: Arc::new(AtomicBool::new(true)),
            }))
            .body(Body::from(
                serde_json::to_vec(body).expect("serialize request"),
            ))
            .expect("request")
    };
    let scenario: Value = serde_json::from_str(include_str!(
        "../../../fixtures/protocol/runtime-smoke/v1.json"
    ))
    .expect("scenario fixture");
    let mut body = scenario["steps"][0]["body"].clone();
    body["pk"] = json!(session_pk);
    let work = signed(
        json!({"schema": "boole.signer.work.v2", "route": "/submit",
        "familyId": "boole.protocol-invariant.v01", "verifierId": "lean-runner-v01", "fee": "0",
        "requestHash": boole_core::canonical_payload_hash_hex(&body), "nonce": "after-revoke",
        "rewardRecipient": reward, "workPayload": body}),
        &session_key,
    );
    let submit = json!({"body": body, "session": {"submittedBy": session_pk,
        "rewardRecipient": reward, "nonce": "after-revoke", "signedWork": work}});

    if let Some(dir) = std::env::var_os(RESTART) {
        let node = LocalNodeState::from_config(make_config(Path::new(&dir)))
            .expect("fresh-process recovery");
        assert!(
            node.session_store
                .as_ref()
                .expect("sessions")
                .get(&session_pk)
                .expect("session")
                .revoked
        );
        let router = build_router(AppState {
            inner: Arc::new(RwLock::new(node)),
            rate_limiter: None,
        });
        let response = router
            .clone()
            .oneshot(post("/submit", &submit))
            .await
            .expect("restarted submit");
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let result: Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(
            result["reason"], "session_revoked",
            "restart authorizes from recovered disk state"
        );
        let response = router
            .oneshot(
                Request::builder()
                    .uri("/ready")
                    .body(Body::empty())
                    .expect("ready"),
            )
            .await
            .expect("restarted ready");
        assert_eq!(response.status(), StatusCode::OK);
        return;
    }

    let affected_dir =
        crate::durability::PrivateTempDir::new("boole-revoke-fenced-node").expect("fixture");
    let other_dir =
        crate::durability::PrivateTempDir::new("boole-revoke-independent-node").expect("fixture");
    let state = Arc::new(RwLock::new(
        LocalNodeState::from_config(make_config(affected_dir.path())).expect("affected boot"),
    ));
    let other_state = Arc::new(RwLock::new(
        LocalNodeState::from_config(make_config(other_dir.path())).expect("independent boot"),
    ));
    let router = build_router(AppState {
        inner: state.clone(),
        rate_limiter: None,
    });
    let other = build_router(AppState {
        inner: other_state,
        rate_limiter: None,
    });
    let register = signed(
        json!({"schema": "boole.sessions.register.v1", "currentHeight": 0,
        "validBefore": now_unix_ms() / 1000 + 60, "nonce": "register",
        "session": {"sessionPk": session_pk, "ownerPk": owner.pk_hex(), "agentPk": "cc".repeat(32),
            "fixedRewardRecipient": reward, "allowedFamilyRoot": "dd".repeat(32), "maxFeePerRequest": "12",
            "activationHeight": 0, "expiryHeight": 100, "revoked": false, "policyHash": "dd".repeat(32)}}),
        &owner,
    );
    for target in [&router, &other] {
        let response = target
            .clone()
            .oneshot(post("/sessions", &register))
            .await
            .expect("register");
        assert_eq!(response.status(), StatusCode::OK);
    }
    let revoke = signed(
        json!({"schema": "boole.sessions.revoke.v1", "sessionPk": session_pk,
        "height": 0, "validBefore": now_unix_ms() / 1000 + 60, "nonce": "revoke"}),
        &owner,
    );
    crate::durability::fail_next_append(crate::durability::AppendFault {
        fail_sync: true,
        fail_rollback: true,
        ..Default::default()
    });
    let response = router
        .clone()
        .oneshot(post(&format!("/sessions/{session_pk}/revoke"), &revoke))
        .await
        .expect("uncertain revoke");
    assert!(!response.status().is_success());
    assert!(
        !state
            .read()
            .await
            .session_store
            .as_ref()
            .expect("sessions")
            .get(&session_pk)
            .expect("session")
            .revoked,
        "the fault leaves authorization memory stale, so the whole node must be fenced"
    );
    let before_files: Vec<_> = [
        "blocks.ndjson",
        "rewards.ndjson",
        "submit-nonces.ndjson",
        "sessions.ndjson",
    ]
    .into_iter()
    .map(|name| (name, std::fs::read(affected_dir.path().join(name)).ok()))
    .collect();
    let response = router
        .clone()
        .oneshot(post("/submit", &submit))
        .await
        .expect("submit after uncertainty");
    assert_eq!(
        response.status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "stale authorization must not admit a signed submit into another ledger"
    );
    assert_eq!(state.read().await.runtime.cached_block_count(), 0);
    for (name, before) in before_files {
        assert_eq!(
            std::fs::read(affected_dir.path().join(name)).ok(),
            before,
            "{name} must not mutate"
        );
    }
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/ready")
                .body(Body::empty())
                .expect("ready"),
        )
        .await
        .expect("affected ready");
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let readiness: Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(readiness["reason"], "storage_write_indeterminate");
    assert_eq!(readiness["checks"]["storage_writes_determinate"], false);
    let response = other
        .oneshot(post("/submit", &submit))
        .await
        .expect("independent submit");
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "disjoint node remains writable"
    );
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let result: Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(
        result["accepted"], true,
        "control proves this signed submit is valid"
    );
    drop(router);
    drop(state);
    let restarted = std::process::Command::new(std::env::current_exe().expect("test binary"))
        .args(["--exact", "local_node::storage_tests::uncertain_session_revoke_fences_only_its_node_until_restart", "--nocapture"])
        .env(RESTART, affected_dir.path()).output().expect("restart process");
    assert!(
        restarted.status.success(),
        "restart failed:\n{}\n{}",
        String::from_utf8_lossy(&restarted.stdout),
        String::from_utf8_lossy(&restarted.stderr)
    );
}

#[test]
fn boot_keeps_node_storage_out_of_checker_and_family_input_trees() {
    for case in [
        "checker-source",
        "checker-new-file",
        "family-new-file",
        "checker-state",
        "checker-absent-root",
    ] {
        let dir =
            crate::durability::PrivateTempDir::new("boole-input-tree-collision").expect("fixture");
        let tree = dir.path().join("input-tree");
        if case != "checker-absent-root" {
            std::fs::create_dir(&tree).expect("input tree");
        }
        let mut cfg = config(dir.path());
        if case == "family-new-file" {
            cfg.family_manifests_dir = Some(tree.clone());
        } else {
            cfg.lean_checker_dir = Some(tree.clone());
            cfg.lean_checker_disabled = false;
        }
        let target = if case == "checker-source" {
            tree.join("lakefile.lean")
        } else if case == "checker-state" {
            tree.join("state/state.manifest.json")
        } else if case == "checker-absent-root" {
            tree.clone()
        } else {
            tree.join("new-ledger.json")
        };
        if case == "checker-source" {
            std::fs::write(&target, b"import Lake\nopen Lake DSL\n").expect("checker source");
        }
        if case == "checker-state" {
            cfg.state_dir = Some(tree.join("state"));
        } else {
            cfg.proof_dedup_ledger_path = Some(target.clone());
        }
        let before = std::fs::read(&target).ok();
        let result = LocalNodeState::from_config(cfg);
        assert_eq!(
            std::fs::read(&target).ok(),
            before,
            "{case} must preserve input tree contents"
        );
        assert!(
            matches!(result, Err(ref error) if error.to_string().contains("storage role collision")),
            "{case}"
        );
    }
}

#[cfg(unix)]
#[tokio::test]
async fn input_tree_guard_preserves_symlink_and_missing_checker_readiness_policy() {
    use tower::ServiceExt;
    let dir =
        crate::durability::PrivateTempDir::new("boole-input-tree-readiness").expect("fixture");
    let checker = dir.path().join("checker");
    std::fs::create_dir(&checker).expect("checker fixture directory");
    let alias = dir.path().join("checker-alias");
    std::os::unix::fs::symlink(&checker, &alias).expect("configured input symlink");
    let mut cfg = config(dir.path());
    cfg.lean_checker_dir = Some(alias.clone());
    cfg.lean_checker_disabled = false;
    drop(LocalNodeState::from_config(cfg).expect("symlinked input directory remains allowed"));
    let mut cfg = config(dir.path());
    cfg.lean_checker_dir = Some(alias);
    let ledger = checker.join("new-ledger.json");
    cfg.proof_dedup_ledger_path = Some(ledger.clone());
    assert!(
        matches!(LocalNodeState::from_config(cfg), Err(ref error) if error.to_string().contains("storage role collision")),
        "input directory aliases must not bypass storage separation"
    );
    assert!(!ledger.exists());
    let mut cfg = config(dir.path());
    let missing = dir.path().join("missing-checker");
    cfg.lean_checker_dir = Some(missing.clone());
    cfg.lean_checker_disabled = false;
    let node = LocalNodeState::from_config(cfg)
        .expect("missing checker remains an availability/readiness failure, not a boot refusal");
    assert!(
        !missing.exists(),
        "validation must not materialize an unavailable checker directory"
    );
    let router = build_router(AppState {
        inner: Arc::new(RwLock::new(node)),
        rate_limiter: None,
    });
    let response = router
        .oneshot(
            Request::builder()
                .uri("/ready")
                .body(Body::empty())
                .expect("ready"),
        )
        .await
        .expect("readiness");
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let ready: Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(ready["reason"], "lean_checker_not_configured");
}

#[cfg(unix)]
#[test]
fn boot_protects_external_targets_of_declared_input_symlinks() {
    for (kind, relative) in [
        ("family", "manifest.json"),
        ("checker", "lakefile.lean"),
        ("checker", "lean-toolchain"),
        ("checker", "lake-manifest.json"),
        ("checker", "Boole/Family/V0Helpers.lean"),
        ("checker", "RELEASE-MANIFEST.json"),
        ("checker-subtree", "BooleCheck"),
        ("checker-dangling", "lakefile.lean"),
        ("family-dangling", "manifest.json"),
    ] {
        let dir =
            crate::durability::PrivateTempDir::new("boole-external-input-alias").expect("fixture");
        let input_tree = dir.path().join("inputs");
        let outside = dir.path().join("outside");
        std::fs::create_dir_all(&input_tree).expect("input tree");
        std::fs::create_dir_all(&outside).expect("external input directory");
        let target = outside.join("input.json");
        if !kind.ends_with("dangling") {
            std::fs::write(&target, b"{\"version\":1,\"work\":[],\"bounties\":[]}\n")
                .expect("external source");
        }
        let link = input_tree.join(relative);
        std::fs::create_dir_all(link.parent().expect("input parent")).expect("source parents");
        if kind == "checker-subtree" {
            std::os::unix::fs::symlink(&outside, &link).expect("source subtree alias");
        } else {
            std::os::unix::fs::symlink(&target, &link).expect("direct input alias");
        }
        let mut cfg = config(&dir.path().join("node"));
        cfg.proof_dedup_ledger_path = Some(target.clone());
        if kind.starts_with("family") {
            cfg.family_manifests_dir = Some(input_tree);
        } else {
            cfg.lean_checker_dir = Some(input_tree);
            cfg.lean_checker_disabled = false;
        }
        let before = std::fs::read(&target).ok();
        let result = LocalNodeState::from_config(cfg);
        assert_eq!(
            std::fs::read(&target).ok(),
            before,
            "{kind}/{relative} target must not change"
        );
        assert!(
            matches!(result, Err(ref error) if error.to_string().contains("storage role collision")),
            "{kind}/{relative}"
        );
    }
}

#[cfg(unix)]
impl FileSizeLimit {
    // This test runs only in an isolated child process, never the suite host.
    #[allow(unsafe_code)]
    fn set(bytes: u64) -> Self {
        let mut prior = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        unsafe {
            assert_eq!(libc::getrlimit(libc::RLIMIT_FSIZE, &mut prior), 0);
            let handler = libc::signal(libc::SIGXFSZ, libc::SIG_IGN);
            let limit = libc::rlimit {
                rlim_cur: bytes as libc::rlim_t,
                rlim_max: prior.rlim_max,
            };
            assert_eq!(libc::setrlimit(libc::RLIMIT_FSIZE, &limit), 0);
            Self { prior, handler }
        }
    }
}

#[cfg(unix)]
impl Drop for FileSizeLimit {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        unsafe {
            assert_eq!(libc::setrlimit(libc::RLIMIT_FSIZE, &self.prior), 0);
            libc::signal(libc::SIGXFSZ, self.handler);
        }
    }
}
