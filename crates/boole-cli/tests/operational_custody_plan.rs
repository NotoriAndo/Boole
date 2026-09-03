//! Operator-facing validation for the real-world release-key custody plan.
//!
//! This test supplies public identifiers and publication locations only. It
//! never creates a signing key or claims that a real custodian/device exists.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use boole_core::canonicalize;
use serde_json::{json, Value};

static FIXTURE_ATTEMPT: AtomicU64 = AtomicU64::new(0);

struct FixtureDir(PathBuf);

impl FixtureDir {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "boole-operational-custody-plan-{}-{nonce}-{}",
            std::process::id(),
            FIXTURE_ATTEMPT.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&path).expect("fixture root");
        Self(path)
    }

    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for FixtureDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn valid_plan() -> Value {
    json!({
        "schema": "boole.operational-key-custody-plan.v1",
        "planId": "boole-production-ceremony-1",
        "environment": "operational-production-readiness",
        "assignments": [
            {
                "role": "product-release",
                "custodyClass": "online-signing",
                "custodianId": "product-release-custodian",
                "deviceId": "product-release-device",
                "deviceClass": "dedicated-online-signer",
                "siteId": "online-site-a"
            },
            {
                "role": "guest-release",
                "custodyClass": "online-signing",
                "custodianId": "guest-release-custodian",
                "deviceId": "guest-release-device",
                "deviceClass": "dedicated-online-signer",
                "siteId": "online-site-b"
            },
            {
                "role": "recovery-a",
                "custodyClass": "offline-recovery",
                "custodianId": "recovery-custodian-a",
                "deviceId": "recovery-device-a",
                "deviceClass": "offline-removable-media",
                "siteId": "recovery-site-a"
            },
            {
                "role": "recovery-b",
                "custodyClass": "offline-recovery",
                "custodianId": "recovery-custodian-b",
                "deviceId": "recovery-device-b",
                "deviceClass": "offline-removable-media",
                "siteId": "recovery-site-b"
            },
            {
                "role": "recovery-c",
                "custodyClass": "offline-recovery",
                "custodianId": "recovery-custodian-c",
                "deviceId": "recovery-device-c",
                "deviceClass": "offline-removable-media",
                "siteId": "recovery-site-c"
            }
        ],
        "publication": {
            "bootstrap": {
                "channelId": "github-release-channel",
                "controlDomainId": "github-release-account",
                "httpsUrl": "https://github.com/NotoriAndo/Boole/releases/download/trust-bootstrap-v1/bootstrap/"
            },
            "recoveryRootPin": {
                "channelId": "independent-root-channel",
                "controlDomainId": "independent-dns-account",
                "httpsUrl": "https://trust.boole.example/recovery-root-v1.sha256"
            },
            "rootPinFormat": "sha256-lowercase-hex",
            "rootPinMustPrecedeAdoption": true
        },
        "approval": {
            "operatorId": "boole-release-owner",
            "approvalId": "custody-plan-approval-1",
            "scope": "ceremony-preparation-only"
        },
        "controls": {
            "privateKeysForbiddenFromRepository": true,
            "recoveryDevicesRemainOffline": true,
            "ceremonyNeedsTwoRecoveryCustodians": true,
            "productionActivationExcluded": true
        }
    })
}

fn run(plan: &Value) -> Output {
    let fixture = FixtureDir::new();
    let path = fixture.join("custody-plan.json");
    fs::write(&path, canonicalize(plan)).expect("write plan");
    Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args([
            "product",
            "verify-operational-custody-plan",
            "--plan",
            path.to_str().expect("plan path"),
        ])
        .output()
        .expect("run boole")
}

#[test]
fn real_cli_accepts_a_complete_separated_plan_without_creating_authority() {
    let output = run(&valid_plan());
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: Value = serde_json::from_slice(&output.stdout).expect("JSON output");
    assert_eq!(result["ok"], true);
    assert_eq!(result["command"], "product.verify-operational-custody-plan");
    assert_eq!(result["result"]["planId"], "boole-production-ceremony-1");
    assert_eq!(
        result["result"]["operatorApprovalId"],
        "custody-plan-approval-1"
    );
    assert_eq!(result["result"]["assignmentCount"], 5);
    assert_eq!(result["result"]["recoveryCustodianCount"], 3);
    assert_eq!(result["result"]["publicationHostCount"], 2);
    assert_eq!(result["result"]["ceremonyPreparationReady"], true);
    assert_eq!(result["result"]["keyGenerationPerformed"], false);
    assert_eq!(result["result"]["authorityGranted"], false);
}

fn assert_rejected(plan: &Value) -> Value {
    let output = run(plan);
    assert!(output.status.code().is_some_and(|code| code != 0));
    let rejection: Value = serde_json::from_slice(&output.stderr).expect("typed rejection");
    assert_eq!(
        rejection["command"],
        "product.verify-operational-custody-plan"
    );
    assert_eq!(rejection["error"]["reason"], "custody-plan-rejected");
    rejection
}

#[test]
fn real_cli_rejects_one_person_or_site_controlling_multiple_recovery_shares() {
    let mut plan = valid_plan();
    plan["assignments"][3]["custodianId"] = plan["assignments"][2]["custodianId"].clone();
    plan["assignments"][3]["siteId"] = plan["assignments"][2]["siteId"].clone();
    let rejection = assert_rejected(&plan);
    assert!(rejection["error"]["message"]
        .as_str()
        .expect("message")
        .contains("distinct custodians and sites"));
}

#[test]
fn real_cli_rejects_a_root_pin_controlled_by_the_bootstrap_publisher() {
    let mut plan = valid_plan();
    plan["publication"]["recoveryRootPin"]["controlDomainId"] =
        plan["publication"]["bootstrap"]["controlDomainId"].clone();
    let rejection = assert_rejected(&plan);
    assert!(rejection["error"]["message"]
        .as_str()
        .expect("message")
        .contains("distinct administrative control domains"));
}

#[test]
fn real_cli_rejects_two_labels_that_still_publish_from_one_https_host() {
    let mut plan = valid_plan();
    plan["publication"]["recoveryRootPin"]["httpsUrl"] =
        Value::String("https://github.com/NotoriAndo/Boole/root.sha256".to_string());
    let rejection = assert_rejected(&plan);
    assert!(rejection["error"]["message"]
        .as_str()
        .expect("message")
        .contains("distinct HTTPS hosts"));
}

#[test]
fn real_cli_rejects_private_key_locations_in_the_public_plan() {
    let mut plan = valid_plan();
    plan["assignments"][0]["privateKeyPath"] = Value::String("/secret/product.key".to_string());
    let rejection = assert_rejected(&plan);
    assert!(rejection["error"]["message"]
        .as_str()
        .expect("message")
        .contains("unknown field `privateKeyPath`"));
}
