// Slice S6 — `boole keys new/list/show` (C2). Local key storage at
// `$BOOLE_KEYS_DIR` (env override) or `$HOME/.boole/keys`, mode 0600,
// atomic tmp+rename. Success → `{ok:true, ...}` on stdout; errors →
// `{ok:false, reason:<kebab>, ...}` on stderr with non-zero exit.
//
// Tests redirect storage via BOOLE_KEYS_DIR so they never touch the user's
// real `~/.boole/keys`. Each test is self-contained: a fresh tempdir per
// invocation, no shared state.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_boole-cli"))
}

fn fresh_tmp(label: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "boole-cli-keys-{}-{}-{}",
        label,
        std::process::id(),
        // Salted with a counter via nanos so multiple temps in one test
        // don't collide.
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn parse_json(bytes: &[u8]) -> serde_json::Value {
    serde_json::from_slice(bytes).unwrap_or_else(|err| {
        panic!(
            "expected JSON: {} (raw={})",
            err,
            String::from_utf8_lossy(bytes)
        )
    })
}

#[test]
fn keys_new_writes_file_with_envelope_and_mode_0600() {
    let dir = fresh_tmp("new");
    let out = cli()
        .env("BOOLE_KEYS_DIR", &dir)
        .args(["keys", "new", "--id", "alice"])
        .output()
        .expect("run boole-cli");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.stderr.is_empty(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    let envelope = parse_json(&out.stdout);
    assert_eq!(envelope["ok"], true);
    let key = &envelope["key"];
    assert_eq!(key["id"], "alice");
    // S13a: keys new defaults to schema v2. W0.2: stdout carries the public
    // view only — `pk`, `createdAt`, `schema`, `id`. The secret `sk` lives
    // exclusively on disk (asserted below).
    assert_eq!(key["schema"], "boole.keys.v2");
    let pk = key["pk"].as_str().expect("pk hex");
    assert_eq!(pk.len(), 64, "pk={pk}");
    assert!(
        pk.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')),
        "pk must be lowercase hex32: {pk}"
    );
    assert!(
        key.get("sk").is_none(),
        "W0.2: stdout must not carry sk: {envelope}"
    );
    let created = key["createdAt"].as_str().expect("createdAt");
    assert!(
        created.ends_with('Z') && created.contains('T'),
        "createdAt must be ISO 8601 UTC: {created}"
    );

    let path = dir.join("alice.json");
    assert!(path.is_file(), "key file missing at {path:?}");
    let mode = std::fs::metadata(&path).expect("stat").permissions().mode();
    // mode_t includes file-type bits in the high nibbles; only the perm
    // bits (last 9 bits) are owner/group/world. The contract is 0600 —
    // owner read+write, no group, no world.
    assert_eq!(mode & 0o777, 0o600, "expected 0600, got {:o}", mode & 0o777);

    // The disk envelope keeps `sk` so `keys sign` can load it; the four
    // public fields round-trip byte-equal with stdout.
    let on_disk: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("read")).expect("disk json");
    assert_eq!(on_disk["id"], "alice");
    assert_eq!(on_disk["pk"], pk);
    assert_eq!(on_disk["createdAt"], created);
    assert_eq!(on_disk["schema"], "boole.keys.v2");
    let disk_sk = on_disk["sk"].as_str().expect("disk sk hex (v2)");
    assert_eq!(disk_sk.len(), 64, "disk sk={disk_sk}");
    assert!(
        disk_sk
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')),
        "disk sk must be lowercase hex32: {disk_sk}"
    );
}

#[test]
fn keys_new_dev_is_deterministic_from_id() {
    let dir_a = fresh_tmp("dev-a");
    let dir_b = fresh_tmp("dev-b");

    let pk_of = |dir: &Path| -> String {
        let out = cli()
            .env("BOOLE_KEYS_DIR", dir)
            .args(["keys", "new", "--id", "alice", "--dev"])
            .output()
            .expect("run boole-cli");
        assert!(
            out.status.success(),
            "stderr={}",
            String::from_utf8_lossy(&out.stderr)
        );
        parse_json(&out.stdout)["key"]["pk"]
            .as_str()
            .expect("pk hex")
            .to_string()
    };

    let pk_a = pk_of(&dir_a);
    let pk_b = pk_of(&dir_b);
    assert_eq!(
        pk_a, pk_b,
        "--dev must produce a deterministic pk from id alone (was {pk_a} vs {pk_b})"
    );
    // Dev keys must NOT collide with non-dev keys for the same id — the
    // dev path is a clearly-marked test seed, not an alias for random.
    let dir_random = fresh_tmp("dev-random");
    let out_random = cli()
        .env("BOOLE_KEYS_DIR", &dir_random)
        .args(["keys", "new", "--id", "alice"])
        .output()
        .expect("run boole-cli");
    assert!(out_random.status.success());
    let pk_random = parse_json(&out_random.stdout)["key"]["pk"]
        .as_str()
        .expect("pk hex")
        .to_string();
    assert_ne!(
        pk_a, pk_random,
        "dev pk must differ from random pk for same id"
    );
}

#[test]
fn keys_new_duplicate_id_emits_key_already_exists_typed_error() {
    let dir = fresh_tmp("dup");
    let first = cli()
        .env("BOOLE_KEYS_DIR", &dir)
        .args(["keys", "new", "--id", "alice"])
        .output()
        .expect("run boole-cli");
    assert!(first.status.success());
    let original_pk = parse_json(&first.stdout)["key"]["pk"]
        .as_str()
        .expect("pk hex")
        .to_string();

    let second = cli()
        .env("BOOLE_KEYS_DIR", &dir)
        .args(["keys", "new", "--id", "alice"])
        .output()
        .expect("run boole-cli");
    assert!(!second.status.success(), "duplicate id must fail");
    assert_eq!(second.status.code(), Some(3), "key_already_exists exits 3");
    assert!(
        second.stdout.is_empty(),
        "typed error must not pollute stdout"
    );
    let envelope = parse_json(&second.stderr);
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["reason"], "key_already_exists");
    assert_eq!(envelope["id"], "alice");

    // The original file must NOT have been overwritten.
    let on_disk: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("alice.json")).expect("read"))
            .expect("disk json");
    assert_eq!(on_disk["pk"], original_pk);
}

#[test]
fn keys_new_dry_run_does_not_write_to_disk() {
    let dir = fresh_tmp("dry");
    let out = cli()
        .env("BOOLE_KEYS_DIR", &dir)
        .args(["keys", "new", "--id", "alice", "--dry-run"])
        .output()
        .expect("run boole-cli");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let envelope = parse_json(&out.stdout);
    assert_eq!(envelope["ok"], true);
    assert_eq!(envelope["key"]["id"], "alice");
    assert_eq!(
        envelope["dryRun"], true,
        "dry-run envelope must self-identify"
    );

    assert!(!dir.join("alice.json").exists(), "dry-run must not write");
    // Directory itself may or may not exist — both are acceptable, but if
    // it exists it must be empty.
    if dir.is_dir() {
        let entries: Vec<_> = std::fs::read_dir(&dir)
            .expect("read dir")
            .collect::<Result<_, _>>()
            .expect("entries");
        assert!(entries.is_empty(), "dry-run must not create siblings");
    }
}

#[test]
fn keys_new_invalid_id_emits_bad_request() {
    let dir = fresh_tmp("invalid");
    for bad_id in ["", "a/b", "../oops", "white space", "dot.in.middle"] {
        let out = cli()
            .env("BOOLE_KEYS_DIR", &dir)
            .args(["keys", "new", "--id", bad_id])
            .output()
            .unwrap_or_else(|_| panic!("run boole-cli for id={bad_id:?}"));
        assert!(!out.status.success(), "id {bad_id:?} should be rejected");
        assert_eq!(
            out.status.code(),
            Some(2),
            "bad_request exits 2 (id={bad_id:?})"
        );
        let envelope = parse_json(&out.stderr);
        assert_eq!(envelope["ok"], false, "id={bad_id:?}");
        assert_eq!(envelope["reason"], "bad_request", "id={bad_id:?}");
    }
}

#[test]
fn keys_list_returns_sorted_keys_array() {
    let dir = fresh_tmp("list");
    // Insert in non-sorted order to prove the listing sorts.
    for id in ["carol", "alice", "bob"] {
        let out = cli()
            .env("BOOLE_KEYS_DIR", &dir)
            .args(["keys", "new", "--id", id])
            .output()
            .expect("run boole-cli");
        assert!(
            out.status.success(),
            "stderr={}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let listed = cli()
        .env("BOOLE_KEYS_DIR", &dir)
        .args(["keys", "list"])
        .output()
        .expect("run boole-cli");
    assert!(listed.status.success());
    let envelope = parse_json(&listed.stdout);
    assert_eq!(envelope["ok"], true);
    let keys = envelope["keys"].as_array().expect("keys array");
    assert_eq!(keys.len(), 3, "envelope={envelope}");
    let ids: Vec<&str> = keys.iter().map(|k| k["id"].as_str().expect("id")).collect();
    assert_eq!(ids, ["alice", "bob", "carol"], "list must sort by id");
    for k in keys {
        // Default schema is v2 after S13a. Existing v1 keys on disk are
        // still listable (regression covered in
        // `keys_list_includes_legacy_v1_envelope_unchanged`).
        // W0.2: stdout carries the public view only — `sk` is disk-only.
        assert_eq!(k["schema"], "boole.keys.v2");
        let pk = k["pk"].as_str().expect("pk");
        assert_eq!(pk.len(), 64);
        assert!(
            k.get("sk").is_none(),
            "W0.2: list stdout must not carry sk: {k}"
        );
    }

    // Disk envelopes still carry `sk` so `keys sign` works.
    for id in ["alice", "bob", "carol"] {
        let path = dir.join(format!("{id}.json"));
        let on_disk: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("read"))
                .expect("disk json");
        let disk_sk = on_disk["sk"].as_str().expect("disk sk hex (v2)");
        assert_eq!(disk_sk.len(), 64);
    }
}

#[test]
fn keys_list_includes_legacy_v1_envelope_unchanged() {
    // Stage a v1 envelope by hand (no `sk` field, schema v1). `keys list`
    // must surface it byte-equal to disk so operators can audit pre-S13a
    // keys without forcing migration.
    let dir = fresh_tmp("legacy-v1");
    std::fs::create_dir_all(&dir).expect("mkdir");
    let v1_envelope = serde_json::json!({
        "schema": "boole.keys.v1",
        "id": "ancient",
        "pk": "00".repeat(32),
        "createdAt": "2025-01-01T00:00:00Z",
    });
    std::fs::write(
        dir.join("ancient.json"),
        serde_json::to_string_pretty(&v1_envelope).expect("serialize v1"),
    )
    .expect("write v1 file");

    let listed = cli()
        .env("BOOLE_KEYS_DIR", &dir)
        .args(["keys", "list"])
        .output()
        .expect("run boole-cli");
    assert!(
        listed.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&listed.stderr)
    );
    let envelope = parse_json(&listed.stdout);
    let keys = envelope["keys"].as_array().expect("keys array");
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0]["schema"], "boole.keys.v1");
    assert_eq!(keys[0]["id"], "ancient");
    assert!(
        keys[0].get("sk").is_none(),
        "v1 envelope must not synthesize sk"
    );
}

#[test]
fn keys_list_with_empty_or_missing_dir_returns_empty_array() {
    // Missing directory entirely — the contract is "empty list, not an
    // error". A user who never ran `keys new` should still be able to
    // `keys list` without crashing.
    let dir = fresh_tmp("missing");
    assert!(!dir.exists());
    let out = cli()
        .env("BOOLE_KEYS_DIR", &dir)
        .args(["keys", "list"])
        .output()
        .expect("run boole-cli");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let envelope = parse_json(&out.stdout);
    assert_eq!(envelope["ok"], true);
    assert_eq!(envelope["keys"], serde_json::json!([]));

    // Empty directory variant — same contract.
    let empty_dir = fresh_tmp("empty");
    std::fs::create_dir_all(&empty_dir).expect("mkdir");
    let out2 = cli()
        .env("BOOLE_KEYS_DIR", &empty_dir)
        .args(["keys", "list"])
        .output()
        .expect("run boole-cli");
    assert!(out2.status.success());
    assert_eq!(parse_json(&out2.stdout)["keys"], serde_json::json!([]));
}

#[test]
fn keys_show_returns_key_envelope_for_existing_id() {
    let dir = fresh_tmp("show-existing");
    let new_out = cli()
        .env("BOOLE_KEYS_DIR", &dir)
        .args(["keys", "new", "--id", "alice"])
        .output()
        .expect("run boole-cli");
    assert!(new_out.status.success());
    let original = parse_json(&new_out.stdout);

    let show_out = cli()
        .env("BOOLE_KEYS_DIR", &dir)
        .args(["keys", "show", "--id", "alice"])
        .output()
        .expect("run boole-cli");
    assert!(show_out.status.success());
    let shown = parse_json(&show_out.stdout);
    assert_eq!(shown["ok"], true);
    // W0.2: both `keys new` and `keys show` now emit the public view, so
    // their `key` sub-objects must still match byte-equal.
    assert_eq!(
        shown["key"], original["key"],
        "show must echo the public view emitted by new"
    );
    assert!(
        shown["key"].get("sk").is_none(),
        "W0.2: show stdout must not carry sk: {shown}"
    );

    // The disk envelope retains `sk` so `keys sign` keeps working.
    let on_disk: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("alice.json")).expect("read"))
            .expect("disk json");
    assert!(
        on_disk["sk"].as_str().is_some(),
        "disk envelope must keep sk"
    );
}

#[test]
fn keys_show_emits_key_not_found_for_missing_id() {
    let dir = fresh_tmp("show-missing");
    std::fs::create_dir_all(&dir).expect("mkdir");
    let out = cli()
        .env("BOOLE_KEYS_DIR", &dir)
        .args(["keys", "show", "--id", "nope"])
        .output()
        .expect("run boole-cli");
    assert!(!out.status.success());
    assert_eq!(out.status.code(), Some(3));
    assert!(out.stdout.is_empty(), "typed error must not pollute stdout");
    let envelope = parse_json(&out.stderr);
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["reason"], "key_not_found");
    assert_eq!(envelope["id"], "nope");
}

// ---------------------------------------------------------------------------
// W0 — Secret-output safety cleanup (RED tests authored in W0.1, made GREEN
// in W0.2 / W0.3). The contract: `boole keys new/list/show` must never echo
// the ed25519 secret seed `sk` to stdout. The on-disk JSON keeps `sk` so
// `boole keys sign` continues to work; an explicit `keys export-secret`
// command (W0.3) is the only path that re-exposes `sk`.
//
// Rationale: agent runtimes shell out to `boole keys ...` and pipe stdout
// through prompts/logs. Any `sk` in stdout is one prompt-injection or one
// log upload away from compromise. The disk file is mode 0600 and stays
// under the operator's control; stdout is not.
// ---------------------------------------------------------------------------

#[test]
fn keys_new_does_not_print_sk_by_default() {
    let dir = fresh_tmp("new-redacted");
    let out = cli()
        .env("BOOLE_KEYS_DIR", &dir)
        .args(["keys", "new", "--id", "alice", "--dev"])
        .output()
        .expect("run keys new");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    let envelope = parse_json(&out.stdout);
    assert_eq!(envelope["ok"], true);
    let key = &envelope["key"];
    assert_eq!(key["id"], "alice");
    assert_eq!(key["schema"], "boole.keys.v2");
    assert!(
        key.get("pk").and_then(|v| v.as_str()).is_some(),
        "public key must still be printed: envelope={envelope}"
    );
    assert!(
        key.get("sk").is_none(),
        "secret key must not be printed: envelope={envelope}"
    );

    // The on-disk file must still carry `sk` so that `boole keys sign`
    // continues to work without a separate KMS lookup. Disk is 0600; stdout
    // is the prompt-injection surface.
    let on_disk: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join("alice.json")).expect("read key file"),
    )
    .expect("disk json");
    assert!(
        on_disk.get("sk").is_some(),
        "MVP disk envelope must keep sk until an encrypted keystore slice replaces it"
    );
}

#[test]
fn keys_show_does_not_print_sk_by_default() {
    let dir = fresh_tmp("show-redacted");
    let create = cli()
        .env("BOOLE_KEYS_DIR", &dir)
        .args(["keys", "new", "--id", "alice", "--dev"])
        .output()
        .expect("run keys new");
    assert!(
        create.status.success(),
        "create stderr={}",
        String::from_utf8_lossy(&create.stderr)
    );

    let out = cli()
        .env("BOOLE_KEYS_DIR", &dir)
        .args(["keys", "show", "--id", "alice"])
        .output()
        .expect("run keys show");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    let envelope = parse_json(&out.stdout);
    assert_eq!(envelope["ok"], true);
    let key = &envelope["key"];
    assert_eq!(key["id"], "alice");
    assert!(key.get("pk").and_then(|v| v.as_str()).is_some());
    assert!(
        key.get("sk").is_none(),
        "secret key must not be printed: envelope={envelope}"
    );
}

#[test]
fn keys_list_does_not_print_sk_by_default() {
    let dir = fresh_tmp("list-redacted");
    let create = cli()
        .env("BOOLE_KEYS_DIR", &dir)
        .args(["keys", "new", "--id", "alice", "--dev"])
        .output()
        .expect("run keys new");
    assert!(create.status.success());

    let out = cli()
        .env("BOOLE_KEYS_DIR", &dir)
        .args(["keys", "list"])
        .output()
        .expect("run keys list");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    let envelope = parse_json(&out.stdout);
    assert_eq!(envelope["ok"], true);
    let keys = envelope["keys"].as_array().expect("keys array");
    assert_eq!(keys.len(), 1);
    assert!(keys[0].get("pk").and_then(|v| v.as_str()).is_some());
    assert!(
        keys[0].get("sk").is_none(),
        "secret key must not be printed: envelope={envelope}"
    );
}

// ---------------------------------------------------------------------------
// W0.3 — `boole keys export-secret --id <id>` is the ONLY path that prints
// `sk` to stdout. The contract:
//   - explicit subcommand (no flag-on-show shortcut),
//   - envelope is marked `"unsafe": true`,
//   - envelope carries a `"warning"` string that mentions "secret",
//   - exit 0 on success,
//   - missing key returns typed `key_not_found` with exit 3 and empty stdout,
//   - bad id returns typed `bad_request` with exit 2,
//   - v1 envelopes (no `sk`) cannot be exported as a secret — typed
//     `no_secret_to_export` exit 3.
// ---------------------------------------------------------------------------

#[test]
fn keys_export_secret_requires_explicit_command_and_prints_warning() {
    let dir = fresh_tmp("export-secret");
    let create = cli()
        .env("BOOLE_KEYS_DIR", &dir)
        .args(["keys", "new", "--id", "alice", "--dev"])
        .output()
        .expect("run keys new");
    assert!(create.status.success());

    let out = cli()
        .env("BOOLE_KEYS_DIR", &dir)
        .args(["keys", "export-secret", "--id", "alice"])
        .output()
        .expect("run keys export-secret");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    let envelope = parse_json(&out.stdout);
    assert_eq!(envelope["ok"], true);
    assert_eq!(envelope["unsafe"], true, "envelope must self-mark unsafe");
    let warning = envelope["warning"]
        .as_str()
        .expect("warning must be a string");
    assert!(
        warning.contains("secret"),
        "warning must mention 'secret': {warning}"
    );
    let key = &envelope["key"];
    assert_eq!(key["id"], "alice");
    let sk = key["sk"].as_str().expect("sk hex");
    assert_eq!(sk.len(), 64, "sk={sk}");
    assert!(
        sk.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')),
        "sk must be lowercase hex32: {sk}"
    );
}

#[test]
fn keys_export_secret_missing_id_emits_key_not_found() {
    let dir = fresh_tmp("export-missing");
    std::fs::create_dir_all(&dir).expect("mkdir");
    let out = cli()
        .env("BOOLE_KEYS_DIR", &dir)
        .args(["keys", "export-secret", "--id", "ghost"])
        .output()
        .expect("run keys export-secret");
    assert!(!out.status.success());
    assert_eq!(out.status.code(), Some(3));
    assert!(out.stdout.is_empty(), "typed error must not pollute stdout");
    let envelope = parse_json(&out.stderr);
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["reason"], "key_not_found");
    assert_eq!(envelope["id"], "ghost");
}

#[test]
fn keys_export_secret_bad_id_emits_bad_request() {
    let dir = fresh_tmp("export-bad-id");
    for bad_id in ["", "a/b", "../oops", "white space"] {
        let out = cli()
            .env("BOOLE_KEYS_DIR", &dir)
            .args(["keys", "export-secret", "--id", bad_id])
            .output()
            .unwrap_or_else(|_| panic!("run for id={bad_id:?}"));
        assert!(!out.status.success(), "id {bad_id:?} should be rejected");
        assert_eq!(out.status.code(), Some(2), "id={bad_id:?}");
        let envelope = parse_json(&out.stderr);
        assert_eq!(envelope["ok"], false, "id={bad_id:?}");
        assert_eq!(envelope["reason"], "bad_request", "id={bad_id:?}");
    }
}

#[test]
fn keys_export_secret_refuses_v1_envelope_without_sk() {
    // Stage a v1 envelope by hand (no `sk` on disk). `export-secret` must
    // refuse rather than emit `sk:null` or empty-string, because callers
    // typically pipe `sk` straight into another tool.
    let dir = fresh_tmp("export-v1");
    std::fs::create_dir_all(&dir).expect("mkdir");
    let v1_envelope = serde_json::json!({
        "schema": "boole.keys.v1",
        "id": "ancient",
        "pk": "00".repeat(32),
        "createdAt": "2025-01-01T00:00:00Z",
    });
    std::fs::write(
        dir.join("ancient.json"),
        serde_json::to_string_pretty(&v1_envelope).expect("serialize v1"),
    )
    .expect("write v1 file");

    let out = cli()
        .env("BOOLE_KEYS_DIR", &dir)
        .args(["keys", "export-secret", "--id", "ancient"])
        .output()
        .expect("run keys export-secret");
    assert!(!out.status.success());
    assert_eq!(out.status.code(), Some(3));
    assert!(out.stdout.is_empty());
    let envelope = parse_json(&out.stderr);
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["reason"], "no_secret_to_export");
    assert_eq!(envelope["id"], "ancient");
    assert_eq!(envelope["schema"], "boole.keys.v1");
}
