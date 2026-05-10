use boole_miner::{AcceptingVerifier, RejectingVerifier, Verifier, VerifyReason};

#[test]
fn test_accepting_verifier_returns_accepted_for_any_input() {
    let v = AcceptingVerifier;
    let r = v.verify("ff".repeat(32).as_str(), 1, "fun xs => x", None);
    assert!(r.accepted);
    assert_eq!(r.reason, VerifyReason::Accepted);
    assert_eq!(r.stderr_tail, "");
}

#[test]
fn test_rejecting_verifier_propagates_supplied_reason() {
    for reason in [
        VerifyReason::EmitFailed,
        VerifyReason::ElaborateFailed,
        VerifyReason::ElaborateTimeout,
        VerifyReason::BinaryNotFound,
    ] {
        let v = RejectingVerifier::new(reason.clone());
        let r = v.verify("aa".repeat(32).as_str(), 1, "x", Some(2));
        assert!(!r.accepted);
        assert_eq!(r.reason, reason);
    }
}

#[test]
fn test_verify_reason_as_str_round_trip() {
    assert_eq!(VerifyReason::Accepted.as_str(), "accepted");
    assert_eq!(VerifyReason::EmitFailed.as_str(), "emit_failed");
    assert_eq!(VerifyReason::ElaborateFailed.as_str(), "elaborate_failed");
    assert_eq!(VerifyReason::ElaborateTimeout.as_str(), "elaborate_timeout");
    assert_eq!(VerifyReason::BinaryNotFound.as_str(), "binary_not_found");
}
