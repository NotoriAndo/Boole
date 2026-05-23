use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::sync::Mutex;

use boole_miner::{
    default_state_path, generate_miner_state, load_state, pubkey_to_address, save_state,
    signing_key_from_state, state_exists, update_config, verifying_key_from_state, ConfigPatch,
    DispatcherConfig, LlmConfig, MinerStateConfig, StateError,
};

fn temp_state_path() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "boole-miner-test-{}-{}",
        std::process::id(),
        rand_index()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("state.json")
}

fn rand_index() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64
}

// Serialize tests that mutate process-global env so they don't race each
// other within the same integration-test binary.
static ENV_LOCK: Mutex<()> = Mutex::new(());

fn sample_config() -> MinerStateConfig {
    MinerStateConfig {
        dispatcher: DispatcherConfig {
            url: "http://localhost:8080".to_string(),
        },
        llm: LlmConfig {
            backend: "noop".to_string(),
            api_key: None,
            model: None,
            base_url: None,
            agent_command: None,
            agent_args: None,
        },
    }
}

#[test]
fn test_pubkey_to_address_is_pk_hex() {
    let pk = [0xab; 32];
    assert_eq!(pubkey_to_address(&pk), "ab".repeat(32));
}

#[test]
fn test_default_state_path_respects_boole_miner_home() {
    let _g = ENV_LOCK.lock().unwrap();
    let old_home = std::env::var("BOOLE_MINER_HOME").ok();
    let old_xdg = std::env::var("XDG_CONFIG_HOME").ok();
    std::env::set_var("BOOLE_MINER_HOME", "/tmp/custom");
    std::env::remove_var("XDG_CONFIG_HOME");
    let p = default_state_path().unwrap();
    assert_eq!(p, PathBuf::from("/tmp/custom/state.json"));
    if let Some(v) = old_home {
        std::env::set_var("BOOLE_MINER_HOME", v);
    } else {
        std::env::remove_var("BOOLE_MINER_HOME");
    }
    if let Some(v) = old_xdg {
        std::env::set_var("XDG_CONFIG_HOME", v);
    }
}

#[test]
fn test_default_state_path_falls_back_to_xdg_config_home() {
    let _g = ENV_LOCK.lock().unwrap();
    let old_home = std::env::var("BOOLE_MINER_HOME").ok();
    let old_xdg = std::env::var("XDG_CONFIG_HOME").ok();
    let old_boole = std::env::var("BOOLE_HOME").ok();
    std::env::remove_var("BOOLE_MINER_HOME");
    std::env::remove_var("BOOLE_HOME");
    std::env::set_var("XDG_CONFIG_HOME", "/tmp/xdg");
    let p = default_state_path().unwrap();
    assert_eq!(p, PathBuf::from("/tmp/xdg/boole-miner/state.json"));
    if let Some(v) = old_home {
        std::env::set_var("BOOLE_MINER_HOME", v);
    }
    if let Some(v) = old_xdg {
        std::env::set_var("XDG_CONFIG_HOME", v);
    } else {
        std::env::remove_var("XDG_CONFIG_HOME");
    }
    if let Some(v) = old_boole {
        std::env::set_var("BOOLE_HOME", v);
    }
}

// P2.3 — BOOLE_HOME is the workspace-wide root that boole-cli already
// honors via boole_core::paths::boole_home_root() for keys/sessions/
// signer-nonces dirs. boole-miner gets the same layer so an operator
// who sets `BOOLE_HOME=/var/lib/boole` finds the miner state under that
// root without needing a separate `BOOLE_MINER_HOME` override.
//
// Precedence (most specific wins):
//   1. BOOLE_MINER_HOME              -> $BOOLE_MINER_HOME/state.json
//   2. BOOLE_HOME                    -> $BOOLE_HOME/miner/state.json
//   3. XDG_CONFIG_HOME               -> $XDG_CONFIG_HOME/boole-miner/state.json
//   4. $HOME                         -> $HOME/.config/boole-miner/state.json
#[test]
fn test_default_state_path_uses_boole_home_when_no_more_specific_override() {
    let _g = ENV_LOCK.lock().unwrap();
    let old_miner = std::env::var("BOOLE_MINER_HOME").ok();
    let old_xdg = std::env::var("XDG_CONFIG_HOME").ok();
    let old_boole = std::env::var("BOOLE_HOME").ok();
    std::env::remove_var("BOOLE_MINER_HOME");
    std::env::remove_var("XDG_CONFIG_HOME");
    std::env::set_var("BOOLE_HOME", "/srv/boole");
    let p = default_state_path().unwrap();
    assert_eq!(p, PathBuf::from("/srv/boole/miner/state.json"));
    if let Some(v) = old_miner {
        std::env::set_var("BOOLE_MINER_HOME", v);
    }
    if let Some(v) = old_xdg {
        std::env::set_var("XDG_CONFIG_HOME", v);
    }
    if let Some(v) = old_boole {
        std::env::set_var("BOOLE_HOME", v);
    } else {
        std::env::remove_var("BOOLE_HOME");
    }
}

#[test]
fn test_default_state_path_boole_miner_home_wins_over_boole_home() {
    let _g = ENV_LOCK.lock().unwrap();
    let old_miner = std::env::var("BOOLE_MINER_HOME").ok();
    let old_xdg = std::env::var("XDG_CONFIG_HOME").ok();
    let old_boole = std::env::var("BOOLE_HOME").ok();
    std::env::set_var("BOOLE_MINER_HOME", "/explicit");
    std::env::set_var("BOOLE_HOME", "/srv/boole");
    std::env::remove_var("XDG_CONFIG_HOME");
    let p = default_state_path().unwrap();
    assert_eq!(p, PathBuf::from("/explicit/state.json"));
    if let Some(v) = old_miner {
        std::env::set_var("BOOLE_MINER_HOME", v);
    } else {
        std::env::remove_var("BOOLE_MINER_HOME");
    }
    if let Some(v) = old_xdg {
        std::env::set_var("XDG_CONFIG_HOME", v);
    }
    if let Some(v) = old_boole {
        std::env::set_var("BOOLE_HOME", v);
    } else {
        std::env::remove_var("BOOLE_HOME");
    }
}

#[test]
fn test_default_state_path_boole_home_wins_over_xdg_config_home() {
    let _g = ENV_LOCK.lock().unwrap();
    let old_miner = std::env::var("BOOLE_MINER_HOME").ok();
    let old_xdg = std::env::var("XDG_CONFIG_HOME").ok();
    let old_boole = std::env::var("BOOLE_HOME").ok();
    std::env::remove_var("BOOLE_MINER_HOME");
    std::env::set_var("BOOLE_HOME", "/srv/boole");
    std::env::set_var("XDG_CONFIG_HOME", "/tmp/xdg");
    let p = default_state_path().unwrap();
    assert_eq!(p, PathBuf::from("/srv/boole/miner/state.json"));
    if let Some(v) = old_miner {
        std::env::set_var("BOOLE_MINER_HOME", v);
    }
    if let Some(v) = old_xdg {
        std::env::set_var("XDG_CONFIG_HOME", v);
    } else {
        std::env::remove_var("XDG_CONFIG_HOME");
    }
    if let Some(v) = old_boole {
        std::env::set_var("BOOLE_HOME", v);
    } else {
        std::env::remove_var("BOOLE_HOME");
    }
}

#[test]
fn test_generate_miner_state_emits_valid_keypair_and_address() {
    let cfg = sample_config();
    let s = generate_miner_state(cfg, "2026-05-09T00:00:00Z");
    assert_eq!(s.schema_version, 1);
    assert_eq!(s.created_at, "2026-05-09T00:00:00Z");
    assert_eq!(s.sk.len(), 64);
    assert_eq!(s.pk.len(), 64);
    assert_eq!(s.address, s.pk);
    let signing = signing_key_from_state(&s).unwrap();
    let derived_pk = hex::encode(signing.verifying_key().to_bytes());
    assert_eq!(derived_pk, s.pk);
}

#[test]
fn test_save_state_writes_mode_0600_atomic() {
    let path = temp_state_path();
    let s = generate_miner_state(sample_config(), "2026-05-09T00:00:00Z");
    save_state(&s, &path).unwrap();
    assert!(state_exists(&path));
    let meta = std::fs::metadata(&path).unwrap();
    let mode = meta.mode() & 0o777;
    assert_eq!(mode, 0o600, "expected 0600, got {mode:o}");
}

#[test]
fn test_save_then_load_round_trips_state() {
    let path = temp_state_path();
    let s = generate_miner_state(sample_config(), "2026-05-09T00:00:00Z");
    save_state(&s, &path).unwrap();
    let loaded = load_state(&path).unwrap();
    assert_eq!(loaded, s);
}

#[test]
fn test_load_state_rejects_unsupported_schema_version() {
    let path = temp_state_path();
    let bogus = serde_json::json!({
        "schemaVersion": 99,
        "sk": "00".repeat(32),
        "pk": "11".repeat(32),
        "address": "11".repeat(32),
        "createdAt": "2026-05-09T00:00:00Z",
        "config": {
            "dispatcher": {"url": "http://x"},
            "llm": {"backend": "noop"}
        }
    });
    std::fs::write(&path, serde_json::to_vec_pretty(&bogus).unwrap()).unwrap();
    let err = load_state(&path).unwrap_err();
    assert!(matches!(err, StateError::UnsupportedSchema(99)));
}

#[test]
fn test_save_state_refuses_to_persist_unsupported_schema() {
    let path = temp_state_path();
    let mut s = generate_miner_state(sample_config(), "2026-05-09T00:00:00Z");
    s.schema_version = 99;
    let err = save_state(&s, &path).unwrap_err();
    assert!(matches!(err, StateError::UnsupportedSchema(99)));
    assert!(!state_exists(&path));
}

#[test]
fn test_update_config_preserves_keypair() {
    let path = temp_state_path();
    let original = generate_miner_state(sample_config(), "2026-05-09T00:00:00Z");
    save_state(&original, &path).unwrap();
    let updated = update_config(
        ConfigPatch {
            dispatcher_url: Some("http://other:9090".to_string()),
            llm: None,
        },
        &path,
    )
    .unwrap();
    assert_eq!(updated.sk, original.sk);
    assert_eq!(updated.pk, original.pk);
    assert_eq!(updated.config.dispatcher.url, "http://other:9090");
}

#[test]
fn test_verifying_key_from_state_matches_pk_field() {
    let s = generate_miner_state(sample_config(), "2026-05-09T00:00:00Z");
    let vk = verifying_key_from_state(&s).unwrap();
    assert_eq!(hex::encode(vk.to_bytes()), s.pk);
}
