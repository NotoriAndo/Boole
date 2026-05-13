use boole_core::{SessionPolicy, SessionState, SignerRequest};

const PK_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const PK_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const PK_C: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const PK_D: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const ROOT: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";

#[test]
fn session_state_accepts_minimal_consensus_fields() {
    let state = SessionState {
        session_pk: PK_A.to_string(),
        owner_pk: PK_B.to_string(),
        agent_pk: PK_C.to_string(),
        fixed_reward_recipient: PK_D.to_string(),
        allowed_family_root: ROOT.to_string(),
        max_fee_per_request: "12".to_string(),
        activation_height: 10,
        expiry_height: 100,
        revoked: false,
        policy_hash: ROOT.to_string(),
    };
    state.validate_at_height(10).expect("valid at activation");
    state.validate_at_height(99).expect("valid before expiry");
}

#[test]
fn session_state_rejects_revoked_or_expired_or_malformed() {
    let mut revoked = SessionState::test_fixture();
    revoked.revoked = true;
    assert!(revoked
        .validate_at_height(10)
        .unwrap_err()
        .to_string()
        .contains("revoked"));

    let expired = SessionState::test_fixture();
    assert!(expired
        .validate_at_height(expired.expiry_height)
        .unwrap_err()
        .to_string()
        .contains("expired"));

    let mut malformed = SessionState::test_fixture();
    malformed.session_pk = "not-hex".to_string();
    assert!(malformed
        .validate_at_height(10)
        .unwrap_err()
        .to_string()
        .contains("sessionPk"));
}

#[test]
fn session_state_rejects_collapsed_role_keys() {
    let mut same = SessionState::test_fixture();
    same.owner_pk = same.session_pk.clone();
    assert!(same
        .validate_at_height(10)
        .unwrap_err()
        .to_string()
        .contains("role keys must be unique"));

    let mut same = SessionState::test_fixture();
    same.agent_pk = same.fixed_reward_recipient.clone();
    assert!(same
        .validate_at_height(10)
        .unwrap_err()
        .to_string()
        .contains("role keys must be unique"));
}

#[test]
fn session_policy_forbids_withdraw_and_transfer() {
    let mut policy = SessionPolicy::test_fixture();
    policy.can_withdraw = true;
    assert!(policy
        .validate()
        .unwrap_err()
        .to_string()
        .contains("canWithdraw=false"));

    let mut policy = SessionPolicy::test_fixture();
    policy.can_transfer = true;
    assert!(policy
        .validate()
        .unwrap_err()
        .to_string()
        .contains("canTransfer=false"));
}

#[test]
fn signer_request_allowed_by_policy_passes() {
    let policy = SessionPolicy::test_fixture();
    let req = SignerRequest::test_fixture();
    policy.authorize(&req).expect("authorized");
}

#[test]
fn signer_request_denies_unknown_family_or_over_fee() {
    let policy = SessionPolicy::test_fixture();
    let mut req = SignerRequest::test_fixture();
    req.family_id = "unknown.family".to_string();
    assert!(policy
        .authorize(&req)
        .unwrap_err()
        .to_string()
        .contains("family"));

    let policy = SessionPolicy::test_fixture();
    let mut req = SignerRequest::test_fixture();
    req.fee = "999".to_string();
    assert!(policy
        .authorize(&req)
        .unwrap_err()
        .to_string()
        .contains("fee"));
}

#[test]
fn signer_request_denies_unknown_route_or_verifier_or_bad_request_hash_or_empty_nonce() {
    let policy = SessionPolicy::test_fixture();

    let mut req = SignerRequest::test_fixture();
    req.route = "/withdraw".to_string();
    assert!(policy
        .authorize(&req)
        .unwrap_err()
        .to_string()
        .contains("route"));

    let mut req = SignerRequest::test_fixture();
    req.verifier_id = "unknown-verifier".to_string();
    assert!(policy
        .authorize(&req)
        .unwrap_err()
        .to_string()
        .contains("verifier"));

    let mut req = SignerRequest::test_fixture();
    req.request_hash = "not-hex".to_string();
    assert!(policy
        .authorize(&req)
        .unwrap_err()
        .to_string()
        .contains("requestHash"));

    let mut req = SignerRequest::test_fixture();
    req.nonce = "   ".to_string();
    assert!(policy
        .authorize(&req)
        .unwrap_err()
        .to_string()
        .contains("nonce"));
}

#[test]
fn signer_request_denies_unsafe_policy_capabilities_and_uppercase_hash() {
    let req = SignerRequest::test_fixture();

    let mut policy = SessionPolicy::test_fixture();
    policy.can_withdraw = true;
    assert!(policy
        .authorize(&req)
        .unwrap_err()
        .to_string()
        .contains("canWithdraw=false"));

    let mut policy = SessionPolicy::test_fixture();
    policy.can_transfer = true;
    assert!(policy
        .authorize(&req)
        .unwrap_err()
        .to_string()
        .contains("canTransfer=false"));

    let policy = SessionPolicy::test_fixture();
    let mut req = SignerRequest::test_fixture();
    req.request_hash =
        "DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD".to_string();
    assert!(policy
        .authorize(&req)
        .unwrap_err()
        .to_string()
        .contains("requestHash"));
}

#[test]
fn session_state_rejects_lifetime_exceeding_max() {
    // Gap G4: a session that is "active for too long" must be refused at
    // register-time so a compromised session's revocation-propagation window
    // is bounded.
    let mut state = SessionState::test_fixture();
    state.activation_height = 0;
    state.expiry_height = boole_core::session_policy::MAX_SESSION_LIFETIME_BLOCKS + 1;
    assert!(state
        .validate_at_height(0)
        .unwrap_err()
        .to_string()
        .contains("lifetime exceeds"));
}
