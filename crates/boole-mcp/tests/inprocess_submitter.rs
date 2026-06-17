//! P2.1 closure (slice 48) — `InProcessSubmitter` trait impl for the
//! `boole-miner` `Submitter` trait.
//!
//! Per master plan §6.5 P2.1 closure criterion 1: boole-mcp must drive
//! a full mining round-trip on a fixture testnet without HTTP loopback
//! to boole-node. Slice 47 landed the `ChainHeadFetcher` half; this
//! slice lands the matching `Submitter` half. The mining-loop wiring
//! and `boole.mine`/`boole.status` tools ride on a follow-up slice.
//!
//! `InProcessSubmitter` is intentionally narrow:
//!   * pins one `AnnounceTicketResult` and one `SubmitResult` so the
//!     mining loop sees a deterministic response,
//!   * captures every `announce_ticket` / `submit` call so the test
//!     harness (and the future `boole.mine` tool) can assert what
//!     shares/blocks the miner actually emitted.

use std::sync::Arc;

use boole_mcp::{CapturedAnnounce, CapturedSubmit, InProcessSubmitter};
use boole_miner::{
    AnnounceTicketInputs, AnnounceTicketResult, SubmitInputs, SubmitResult, Submitter,
};

fn observed_announce() -> AnnounceTicketResult {
    AnnounceTicketResult::Observed {
        hash_hex: "0xticket".to_string(),
    }
}

fn accepted_submit() -> SubmitResult {
    SubmitResult::Accepted {
        share_hash_hex: "0xshare".to_string(),
    }
}

#[test]
fn inprocess_submitter_returns_pinned_results() {
    let submitter = InProcessSubmitter::new(observed_announce(), accepted_submit());

    let announce_got = submitter.announce_ticket(AnnounceTicketInputs {
        c_hex: "c1",
        pk_hex: "pk1",
        n_hex: "n1",
    });
    assert_eq!(announce_got, observed_announce());

    let submit_got = submitter.submit(SubmitInputs {
        c_hex: "c1",
        pk_hex: "pk1",
        n_hex: "n1",
        j_hex: "j1",
        nonce_s_hex: "nonce1",
        canon_bytes: b"canon",
        seed_hex: "",
    });
    assert_eq!(submit_got, accepted_submit());
}

#[test]
fn inprocess_submitter_captures_announce_inputs() {
    let submitter = InProcessSubmitter::new(observed_announce(), accepted_submit());

    submitter.announce_ticket(AnnounceTicketInputs {
        c_hex: "c1",
        pk_hex: "pk1",
        n_hex: "n1",
    });
    submitter.announce_ticket(AnnounceTicketInputs {
        c_hex: "c2",
        pk_hex: "pk2",
        n_hex: "n2",
    });

    let captured = submitter.captured_announces();
    assert_eq!(captured.len(), 2);
    assert_eq!(
        captured[0],
        CapturedAnnounce {
            c_hex: "c1".to_string(),
            pk_hex: "pk1".to_string(),
            n_hex: "n1".to_string(),
        }
    );
    assert_eq!(
        captured[1],
        CapturedAnnounce {
            c_hex: "c2".to_string(),
            pk_hex: "pk2".to_string(),
            n_hex: "n2".to_string(),
        }
    );
}

#[test]
fn inprocess_submitter_captures_submit_inputs_including_canon_bytes() {
    let submitter = InProcessSubmitter::new(observed_announce(), accepted_submit());

    submitter.submit(SubmitInputs {
        c_hex: "cX",
        pk_hex: "pkX",
        n_hex: "nX",
        j_hex: "jX",
        nonce_s_hex: "nonceX",
        canon_bytes: b"\x00\x01\x02\xff",
        seed_hex: "",
    });

    let captured = submitter.captured_submits();
    assert_eq!(captured.len(), 1);
    assert_eq!(
        captured[0],
        CapturedSubmit {
            c_hex: "cX".to_string(),
            pk_hex: "pkX".to_string(),
            n_hex: "nX".to_string(),
            j_hex: "jX".to_string(),
            nonce_s_hex: "nonceX".to_string(),
            canon_bytes: vec![0x00, 0x01, 0x02, 0xff],
        }
    );
}

#[test]
fn inprocess_submitter_is_trait_object_safe_via_arc_dyn() {
    // Mining loop holds the submitter behind `Arc<dyn Submitter>`, so
    // this test pins the trait-object cast at the public surface.
    let submitter: Arc<dyn Submitter> = Arc::new(InProcessSubmitter::new(
        observed_announce(),
        accepted_submit(),
    ));

    let got = submitter.submit(SubmitInputs {
        c_hex: "c",
        pk_hex: "pk",
        n_hex: "n",
        j_hex: "j",
        nonce_s_hex: "ns",
        canon_bytes: b"",
        seed_hex: "",
    });
    assert_eq!(got, accepted_submit());
}
