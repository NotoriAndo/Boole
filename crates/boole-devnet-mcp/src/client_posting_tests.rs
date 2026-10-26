//! Crash-window tests use the real client loop and private-store publication.
use super::*;
#[path = "../tests/support/posting.rs"]
#[allow(dead_code)]
mod signing;
use crate::posting::SignedPosting;
use boole_devnet_archive::durability::PrivateTempDir;

#[derive(Clone)]
struct Source {
    finalized: u64,
    rows: BTreeMap<u64, (L1Block, Vec<InboxLog>)>,
    posting_unavailable: bool,
}
impl L1Source for Source {
    fn restore_posting_proof(&mut self, _: &str, _: u64) -> Result<()> {
        if self.posting_unavailable {
            return Err(boole_devnet_archive::error::availability(
                5,
                "archive-tx-proof",
                "withheld receipt",
            ));
        }
        Ok(())
    }
    fn finalized(&mut self) -> Result<u64> {
        Ok(self.finalized)
    }
    fn block(&mut self, n: u64) -> Result<L1Block> {
        Ok(self.rows[&n].0.clone())
    }
    fn inbox_logs(&mut self, hash: &str) -> Result<Vec<InboxLog>> {
        Ok(self
            .rows
            .values()
            .find(|r| r.0.hash == hash)
            .context("test block")?
            .1
            .clone())
    }
}
#[derive(Default)]
struct Posting {
    tamper_at_head: Option<(PathBuf, Vec<u8>)>,
    receipt: Option<u64>,
    consumed: bool,
    head: Option<u64>,
    interrupt_signing: bool,
    failure_target: Option<PathBuf>,
    fail_after_rename: bool,
    signing_marker: Option<PathBuf>,
    sends: u64,
    tx: String,
    ambiguous: bool,
    rebroadcasts: Vec<String>,
}
impl Poster for Posting {
    fn nonce_observation(
        &mut self,
        meta: &crate::posting::PostingMetadata,
    ) -> crate::nonce::NonceObservation {
        if self.consumed {
            crate::nonce::NonceObservation {
                state: crate::nonce::NonceState::NonceConsumed,
                inclusion: crate::nonce::Inclusion::Unresolved,
                finalized_height: Some(15),
                finalized_hash: Some(hex_string(&[15; 32])),
                account_nonce: Some(meta.nonce + 1),
                reason: None,
            }
        } else {
            crate::nonce::NonceObservation::unavailable(crate::nonce::UnavailableReason::NoJournal)
        }
    }

    fn head(&mut self) -> Result<u64> {
        if let Some((path, raw)) = self.tamper_at_head.take() {
            std::fs::write(path, raw)?;
        }
        Ok(self.head.unwrap_or(5))
    }
    fn post(&mut self, _: u8, _: &[u8]) -> Result<String> {
        unreachable!()
    }
    fn included(&mut self, _: &str) -> Result<Option<u64>> {
        Ok(self.receipt)
    }
    fn post_durable(
        &mut self,
        kind: u8,
        body: &[u8],
        persist: &mut dyn FnMut(&SignedPosting) -> Result<()>,
    ) -> Result<String> {
        if let Some(marker) = &self.signing_marker {
            std::fs::write(marker, b"durable intent, signing not completed")?;
            std::thread::sleep(std::time::Duration::from_secs(30));
            bail!("sign-window child was not killed");
        }
        assert!(
            !self.interrupt_signing,
            "simulated process death in signing window"
        );
        let (raw, account) = signing::signed(kind, body, 7 + self.sends);
        let signed = SignedPosting::authenticate(
            raw,
            kind,
            body,
            &account,
            &1_000_000_000u32.to_be_bytes(),
        )?;
        persist(&signed)?;
        self.sends += 1;
        self.tx = signed.metadata.transaction_hash;
        if let Some(path) = self.failure_target.take() {
            if self.fail_after_rename {
                crate::store::FAIL_AFTER_RENAME.with(|slot| *slot.borrow_mut() = Some(path));
            } else {
                crate::store::FAIL_NEXT_WRITE.with(|slot| *slot.borrow_mut() = Some(path));
            }
        }
        if self.ambiguous {
            Err(crate::posting::PostingAmbiguous.into())
        } else {
            Ok(self.tx.clone())
        }
    }
    fn rebroadcast(&mut self, signed: &SignedPosting) -> Result<()> {
        self.rebroadcasts
            .push(hex_string(&boole_derivation::user_tx::keccak(&signed.raw)));
        bail!("nonce too low is not an inclusion verdict")
    }
}
#[test]
fn t81_sign_window_restart_is_definitely_unsent_but_legacy_none_is_ambiguous() -> Result<()> {
    use std::panic::{catch_unwind, AssertUnwindSafe};
    let (_dir, mut client, id) = fixture()?;
    client.poster.interrupt_signing = true;
    assert!(catch_unwind(AssertUnwindSafe(|| client.tick())).is_err());
    let source = client.source.clone();
    let config = client.config.clone();
    let path = client.store.path.clone();
    let account = client.account;
    let saved: Value = client.store.read(&submission_name(id))?;
    assert_eq!(
        saved["sent"]["commit"]["send_state"], "intent",
        "crash window is indistinguishable from a legacy ambiguous send"
    );
    assert_eq!(client.poster.sends, 0);
    drop(client);
    let client = Client::new(
        source.clone(),
        Posting::default(),
        config.clone(),
        account,
        path.clone(),
        ClientOptions::new(30_000_000),
    )?;
    assert_eq!(
        client.submission(id)?["transactions"][0]["send_state"],
        "not-sent"
    );
    let mut saved: Value = client.store.read(&submission_name(id))?;
    saved["sent"]["commit"]["send_state"] = Value::Null;
    client.store.write(&submission_name(id), &saved)?;
    drop(client);
    let client = Client::new(
        source,
        Posting::default(),
        config,
        account,
        path,
        ClientOptions::new(30_000_000),
    )?;
    assert_eq!(
        client.submission(id)?["transactions"][0]["send_state"],
        Value::Null,
        "legacy None was guessed unsent"
    );
    Ok(())
}
fn source_config() -> Result<(Source, Config)> {
    let data: Value = serde_json::from_slice(include_bytes!(
        "../../../fixtures/devnet-m2/rounds-v1.1.json"
    ))?;
    let config: Config = serde_json::from_value(data["config"].clone())?;
    let mut rows = BTreeMap::new();
    for r in data["heights"].as_array().unwrap() {
        let block: L1Block = serde_json::from_value(r["l1"].clone())?;
        rows.insert(
            block.number,
            (block, serde_json::from_value(r["logs"].clone())?),
        );
    }
    let mut genesis = rows[&1].0.clone();
    genesis.number = 0;
    genesis.hash = genesis.parent_hash.clone();
    rows.insert(0, (genesis, vec![]));
    Ok((
        Source {
            finalized: 5,
            rows,
            posting_unavailable: false,
        },
        config,
    ))
}
#[test]
fn finalized_success_without_effect_after_commit_budget_can_sign_again() -> Result<()> {
    let (_dir, mut client, id) = fixture()?;
    client.tick()?;
    let prepared = client.submissions[&id].prepared.clone();
    let row = client.source.rows.get_mut(&6).unwrap();
    row.1.clear();
    for i in 0..=client.config.c_commit {
        let mut body = prepared.commit_body(client.account);
        if i < client.config.c_commit {
            body[53..85].copy_from_slice(&digest(&i.to_be_bytes()));
        }
        row.1.push(InboxLog {
            address: boole_derivation::config::INBOX.into(),
            topics: vec![boole_derivation::config::posted_topic()],
            block_hash: row.0.hash.clone(),
            removed: false,
            transaction_index: i,
            log_index: i,
            data: hex_string(&calldata(2, &body)[4..]),
        });
    }
    client.source.finalized = 6;
    client.poster.head = Some(8);
    client.poster.receipt = Some(6);
    client.tick()?;
    assert!(
        !client
            .state
            .candidates
            .contains_key(&(prepared.problem, id)),
        "the last COMMIT should be refused by c_commit"
    );
    assert_eq!(
        client.poster.sends, 2,
        "a finalized successful receipt with no fold effect stalled the stage"
    );
    Ok(())
}
fn fixture() -> Result<(PrivateTempDir, Client<Source, Posting>, Hash)> {
    fixture_parts(1)
}
fn fixture_parts(parts: usize) -> Result<(PrivateTempDir, Client<Source, Posting>, Hash)> {
    let dir = PrivateTempDir::new("m4c5-fix1-client")?;
    let (source, config) = source_config()?;
    let (_, account) = signing::signed(2, b"unused", 7);
    let mut client = Client::new(
        source,
        Posting::default(),
        config.clone(),
        fixed(&account)?,
        dir.path().join("private"),
        ClientOptions::new(30_000_000),
    )?;
    client.sync()?;
    let p = client.state.rounds[&(0, 1)].problem.context("problem")?;
    let compressed = if parts == 1 {
        b"abc".to_vec()
    } else {
        vec![b'a'; PART_SIZE * (parts - 1) + 1]
    };
    let prepared = Prepared::new(
        &config,
        Material {
            problem: p,
            payee: [6; 20],
            salt: [1; 32],
            cancel_secret: [11; 32],
            raw_len: compressed.len() as u64,
            export_digest: [1; 32],
            compressed,
            packages: vec![],
            format: 2,
        },
    )?;
    let id = prepared.commit;
    let raw = json!({"schema_version":2,"config_digest":config.digest(),"prepared":prepared,"cancel_requested":false,"local_check":"not_run","sent":{},"last_error":null});
    let sub: Submission = serde_json::from_value(raw)?;
    client.save(&sub)?;
    Ok((dir, client, id))
}
#[test]
fn t81_damaged_record_is_stage_typed_without_masking_receipt_availability() -> Result<()> {
    let (_dir, mut client, id) = fixture()?;
    client.tick()?;
    let name = format!("posting-{}.json", client.poster.tx.trim_start_matches("0x"));
    std::fs::write(client.store.path.join(name), b"{damaged")?;
    client.source.posting_unavailable = true;
    client.poster.head = Some(7);
    client.tick()?;
    let view = client.submission(id)?;
    assert!(
        view["last_error"]
            .as_str()
            .unwrap()
            .contains("archive-tx-proof"),
        "damaged record masked original receipt availability: {view}"
    );
    assert_eq!(
        view["transactions"][0]["stage_error"],
        "posting-record-damaged"
    );
    assert_eq!(client.poster.sends, 1);
    Ok(())
}
#[test]
fn t81_failed_post_send_save_never_overwrites_durable_prepared() -> Result<()> {
    for after_rename in [false, true] {
        let (_dir, mut client, id) = fixture()?;
        client.poster.failure_target = Some(client.store.path.join(submission_name(id)));
        client.poster.fail_after_rename = after_rename;
        client.tick()?;
        assert_eq!(client.poster.sends, 1);
        let disk: Value = client.store.read(&submission_name(id))?;
        assert_eq!(
            disk["sent"]["commit"]["tx"], client.poster.tx,
            "tick replaced durable posting with stale intent"
        );
        assert_eq!(
            disk["sent"]["commit"]["send_state"],
            if after_rename {
                "acknowledged"
            } else {
                "prepared"
            },
            "tick regressed the published state"
        );
        assert_eq!(
            client.submission(id)?["transactions"][0]["transaction_hash"],
            client.poster.tx
        );
    }
    Ok(())
}

#[test]
fn t81_uncommitted_parts_rebroadcast_only_identical_bytes_with_a_bounded_count() -> Result<()> {
    for ambiguous in [false, true] {
        let (_dir, mut client, id) = open_client(1)?;
        client.poster.ambiguous = ambiguous;
        client.tick()?;
        let hash = client.poster.tx.clone();
        let h = client.source.finalized;
        for head in h + 1..=h + 9 {
            client.poster.head = Some(head);
            client.tick()?;
        }
        assert_eq!(client.poster.sends, 2);
        assert_eq!(client.poster.rebroadcasts, vec![hash; 3]);
        assert_eq!(
            client.submission(id)?["transactions"]
                .as_array()
                .unwrap()
                .iter()
                .find(|v| v["stage"] == "part:0")
                .unwrap()["send_state"],
            if ambiguous {
                "ambiguous"
            } else {
                "acknowledged"
            }
        );
        let source = client.source.clone();
        let config = client.config.clone();
        let account = client.account;
        let path = client.store.path.clone();
        drop(client);
        let mut client = Client::new(
            source,
            Posting {
                head: Some(h + 10),
                ..Posting::default()
            },
            config,
            account,
            path,
            ClientOptions::new(30_000_000),
        )?;
        client.tick()?;
        assert!(
            client.poster.rebroadcasts.is_empty(),
            "restart reset retry cap"
        );
    }
    Ok(())
}

#[test]
#[ignore = "sign-window subprocess fixture only"]
fn t81_sign_window_crash_child() -> Result<()> {
    let path =
        PathBuf::from(std::env::var_os("BOOLE_UNIT_SIGN_PRIVATE").context("child private path")?);
    let marker = PathBuf::from(std::env::var_os("BOOLE_UNIT_SIGN_MARKER").context("child marker")?);
    let (source, config) = source_config()?;
    let (_, account) = signing::signed(2, b"unused", 7);
    let posting = Posting {
        signing_marker: Some(marker),
        ..Posting::default()
    };
    let mut client = Client::new(
        source,
        posting,
        config,
        fixed(&account)?,
        path,
        ClientOptions::new(30_000_000),
    )?;
    client.tick()
}
#[test]
#[cfg(unix)]
fn t81_sigkill_in_sign_window_restarts_as_notsent() -> Result<()> {
    use std::os::unix::process::ExitStatusExt;
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, Instant};
    struct Reap(Child);
    impl Drop for Reap {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let (dir, client, id) = fixture()?;
    let source = client.source.clone();
    let config = client.config.clone();
    let account = client.account;
    let path = client.store.path.clone();
    drop(client);
    let marker = dir.path().join("signing");
    let mut child = Reap(
        Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "client::posting_tests::t81_sign_window_crash_child",
                "--ignored",
                "--nocapture",
            ])
            .env("BOOLE_UNIT_SIGN_PRIVATE", &path)
            .env("BOOLE_UNIT_SIGN_MARKER", &marker)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?,
    );
    let until = Instant::now() + Duration::from_secs(10);
    while !marker.exists() {
        ensure!(child.0.try_wait()?.is_none(), "child exited before signing");
        ensure!(Instant::now() < until, "sign marker timeout");
        std::thread::sleep(Duration::from_millis(5));
    }
    child.0.kill()?;
    assert_eq!(child.0.wait()?.signal(), Some(9));
    let saved: Value = serde_json::from_slice(&std::fs::read(path.join(submission_name(id)))?)?;
    assert_eq!(saved["sent"]["commit"]["send_state"], "intent");
    assert!(saved["sent"]["commit"]["posting"].is_null());
    let client = Client::new(
        source,
        Posting::default(),
        config,
        account,
        path,
        ClientOptions::new(30_000_000),
    )?;
    assert_eq!(
        client.submission(id)?["transactions"][0]["send_state"],
        "not-sent"
    );
    Ok(())
}

#[test]
fn mismatched_recovery_posting_is_typed_and_does_not_skip_later_submissions() -> Result<()> {
    let (_dir, mut client, id) = fixture()?;
    client.tick()?;
    client.poster.head = Some(6);
    client.tick()?;
    assert!(client.submissions[&id].sent["commit"]
        .nonce_observation
        .is_some());
    let original = client.submissions[&id].clone();
    let mut second = original.clone();
    second.prepared = (2..=255u8)
        .find_map(|salt| {
            let p = Prepared::new(
                &client.config,
                Material {
                    problem: original.prepared.problem,
                    payee: [6; 20],
                    salt: [salt; 32],
                    cancel_secret: [11; 32],
                    raw_len: 3,
                    export_digest: [1; 32],
                    compressed: b"abc".to_vec(),
                    packages: vec![],
                    format: 2,
                },
            )
            .ok()?;
            (p.commit > id).then_some(p)
        })
        .context("later submission")?;
    let later = second.prepared.commit;
    second.sent.clear();
    second.observed = None;
    client.save(&second)?;
    let mut bad: Value = client.store.read(&submission_name(id))?;
    bad["sent"]["commit"]["tx"] = json!(hex_string(&[99; 32]));
    let path = client.store.path.join(submission_name(id));
    let before = serde_json::to_vec(&bad)?;
    client.poster.tamper_at_head = Some((path.clone(), before.clone()));
    client.source.posting_unavailable = true;
    client.poster.head = Some(7);
    client.tick()?;
    let view = client.submission(id)?;
    assert_eq!(
        view["progress_error"], "durable-posting-invalid",
        "recovery adopted a tx inconsistent with the signed record: {view}"
    );
    assert!(view["last_error"]
        .as_str()
        .is_some_and(|s| s.contains("durable")));
    assert_eq!(
        std::fs::read(&path)?,
        before,
        "recovery overwrote an invalid durable record"
    );
    assert_eq!(
        client.poster.sends, 2,
        "bad submission skipped a later first send"
    );
    assert!(!client.submissions[&later].sent.is_empty());
    Ok(())
}

#[test]
fn default_durable_poster_refuses_without_calling_the_legacy_send() -> Result<()> {
    struct Legacy(u64);
    impl Poster for Legacy {
        fn head(&mut self) -> Result<u64> {
            Ok(5)
        }
        fn post(&mut self, _: u8, _: &[u8]) -> Result<String> {
            self.0 += 1;
            Ok(hex_string(&[1; 32]))
        }
        fn included(&mut self, _: &str) -> Result<Option<u64>> {
            Ok(None)
        }
    }
    let mut poster = Legacy(0);
    let result = poster.post_durable(2, b"intent", &mut |_| {
        bail!("unexpected persistence callback")
    });
    assert_eq!(poster.0, 0, "default durable poster sent before Prepared");
    assert!(result.unwrap_err().is::<crate::posting::PostingNotSent>());
    Ok(())
}

fn put_inputs(client: &mut Client<Source, Posting>, h: u64, items: Vec<(u8, Vec<u8>)>) {
    let row = client.source.rows.get_mut(&h).unwrap();
    row.1.clear();
    for (i, (kind, body)) in items.into_iter().enumerate() {
        row.1.push(InboxLog {
            address: boole_derivation::config::INBOX.into(),
            topics: vec![boole_derivation::config::posted_topic()],
            block_hash: row.0.hash.clone(),
            removed: false,
            transaction_index: i as u64,
            log_index: i as u64,
            data: hex_string(&calldata(kind, &body)[4..]),
        });
    }
}
fn open_client(parts: usize) -> Result<(PrivateTempDir, Client<Source, Posting>, Hash)> {
    let (dir, mut client, id) = fixture_parts(parts)?;
    client.tick()?;
    let p = client.submissions[&id].prepared.clone();
    let h_o = 6 + client.config.d;
    for h in 6..=h_o {
        put_inputs(&mut client, h, vec![]);
    }
    let account = client.account;
    put_inputs(&mut client, 6, vec![(2, p.commit_body(account))]);
    put_inputs(&mut client, h_o, vec![(3, p.open(account))]);
    client.source.finalized = h_o;
    client.poster.head = Some(h_o);
    client.sync()?;
    assert_eq!(
        client.state.candidates[&(p.problem, id)].status(),
        Status::Open
    );
    Ok((dir, client, id))
}
#[test]
fn client_path_nonce_consumed_stops_part_rebroadcast() -> Result<()> {
    let (_dir, mut client, id) = open_client(1)?;
    client.tick()?;
    let initial = client.poster.sends;
    client.poster.head = Some(client.source.finalized + 2);
    client.poster.consumed = true;
    client.tick()?;
    assert!(
        client.poster.rebroadcasts.is_empty(),
        "consumed nonce still spent a rebroadcast attempt"
    );
    assert_eq!(client.poster.sends, initial);
    assert_eq!(
        client.submissions[&id].sent["part:0"].rebroadcast_attempts,
        0
    );
    client.poster.consumed = false;
    client.poster.head = Some(client.source.finalized + 4);
    client.tick()?;
    assert!(client.poster.rebroadcasts.is_empty());
    let source = client.source.clone();
    let config = client.config.clone();
    let account = client.account;
    let path = client.store.path.clone();
    let h = client.source.finalized;
    drop(client);
    let mut client = Client::new(
        source,
        Posting {
            head: Some(h + 6),
            ..Posting::default()
        },
        config,
        account,
        path,
        ClientOptions::new(30_000_000),
    )?;
    client.tick()?;
    assert!(
        client.poster.rebroadcasts.is_empty(),
        "restart forgot the no-rebroadcast decision for a consumed nonce"
    );
    Ok(())
}

#[test]
fn client_path_first_sends_win_last_window_budget_and_restart_keeps_charge() -> Result<()> {
    let (_dir, mut client, id) = open_client(8)?;
    let end = 6 + client.config.w;
    client.poster.head = Some(end - 3);
    client.tick()?;
    assert_eq!(client.poster.sends, 7);
    client.poster.head = Some(end - 1);
    client.tick()?;
    assert_eq!(
        client.poster.sends, 9,
        "rebroadcasts spent the last head before all first sends"
    );
    assert_eq!(client.poster.rebroadcasts.len(), 4);
    assert_eq!(
        client.posted, client.config.p_blk,
        "rebroadcast was not charged to P_BLK"
    );
    assert!(client.submissions[&id].sent.contains_key("part:7"));
    let source = client.source.clone();
    let config = client.config.clone();
    let account = client.account;
    let path = client.store.path.clone();
    drop(client);
    let mut client = Client::new(
        source,
        Posting {
            head: Some(end - 1),
            ..Posting::default()
        },
        config,
        account,
        path,
        ClientOptions::new(30_000_000),
    )?;
    client.tick()?;
    assert_eq!(client.posted, client.config.p_blk);
    assert_eq!(client.poster.sends, 0);
    assert!(
        client.poster.rebroadcasts.is_empty(),
        "restart forgot rebroadcast budget"
    );
    client.poster.head = Some(end);
    client.tick()?;
    assert!(
        client.poster.rebroadcasts.is_empty(),
        "rebroadcast escaped the window"
    );
    assert_eq!(client.poster.sends, 0);
    Ok(())
}

#[test]
fn client_path_receipt_stops_rebroadcast_and_restart_below_cap_continues() -> Result<()> {
    let (_dir, mut client, id) = open_client(1)?;
    client.tick()?;
    let h = client.source.finalized;
    client.poster.head = Some(h + 2);
    client.poster.receipt = Some(h + 1);
    client.tick()?;
    assert!(client.poster.rebroadcasts.is_empty());
    assert_eq!(client.poster.sends, 2);
    client.poster.receipt = None;
    client.tick()?;
    assert_eq!(client.poster.rebroadcasts.len(), 1);
    let source = client.source.clone();
    let config = client.config.clone();
    let account = client.account;
    let path = client.store.path.clone();
    drop(client);
    let mut client = Client::new(
        source,
        Posting {
            head: Some(h + 4),
            ..Posting::default()
        },
        config,
        account,
        path,
        ClientOptions::new(30_000_000),
    )?;
    client.tick()?;
    assert_eq!(client.poster.rebroadcasts.len(), 1);
    assert_eq!(
        client.submissions[&id].sent["part:0"].rebroadcast_attempts,
        2
    );
    assert_eq!(client.poster.sends, 0);
    Ok(())
}
#[test]
fn client_path_commit_open_cancel_are_excluded_from_rebroadcast() -> Result<()> {
    for stage in ["commit", "open", "cancel"] {
        let (_dir, mut client, id) = fixture()?;
        client.tick()?;
        let mut head = 7;
        if stage != "commit" {
            let body = client.submissions[&id].prepared.commit_body(client.account);
            put_inputs(&mut client, 6, vec![(2, body)]);
            client.source.finalized = 6;
            client.sync()?;
            head = 6 + client.config.d - 1;
            client.poster.head = Some(head);
            if stage == "cancel" {
                client.cancel(id)?;
            }
            client.tick()?;
        }
        let sends = client.poster.sends;
        for next in [head + 2, head + 4] {
            client.poster.head = Some(next);
            client.tick()?;
        }
        assert!(client.submissions[&id].sent.contains_key(stage));
        assert_eq!(
            client.poster.sends, sends,
            "new {stage} signed without a receipt"
        );
        assert!(
            client.poster.rebroadcasts.is_empty(),
            "rebroadcast included {stage}"
        );
    }
    Ok(())
}
