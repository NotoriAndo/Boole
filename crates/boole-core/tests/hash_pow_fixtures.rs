use boole_core::{
    difficulty_weight, digest_to_biguint, min_share_score, parse_biguint_hex, share_hash,
    share_score, submission_pow_hash, submission_pow_ok, ticket, Hex32,
};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Fixture {
    inputs: Inputs,
    expected: Expected,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Inputs {
    c: String,
    pk: String,
    n: String,
    j: String,
    canon_hash: String,
    nonce_s: String,
    #[serde(rename = "T_ticket")]
    t_ticket: String,
    #[serde(rename = "T_submit")]
    t_submit: String,
    #[serde(rename = "T_share")]
    t_share: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Expected {
    ticket: TicketExpected,
    share_hash: String,
    share_hash_int: String,
    share_score: String,
    difficulty_weight: String,
    min_share_score: String,
    submission_pow: SubmissionExpected,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TicketExpected {
    valid: bool,
    hash_bytes: String,
    hash_int: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SubmissionExpected {
    hash_bytes: String,
    hash_int: String,
    ok: bool,
}

#[test]
fn hash_pow_matches_typescript_golden_fixture() {
    let fixture: Fixture =
        serde_json::from_str(include_str!("../../../fixtures/protocol/hash-pow/v1.json"))
            .expect("fixture parses");

    let c = Hex32::from_hex(&fixture.inputs.c).expect("c");
    let pk = Hex32::from_hex(&fixture.inputs.pk).expect("pk");
    let n = Hex32::from_hex(&fixture.inputs.n).expect("n");
    let j = Hex32::from_hex(&fixture.inputs.j).expect("j");
    let canon_hash = Hex32::from_hex(&fixture.inputs.canon_hash).expect("canon hash");
    let nonce_s = Hex32::from_hex(&fixture.inputs.nonce_s).expect("nonceS");
    let t_ticket = parse_biguint_hex(&fixture.inputs.t_ticket).expect("T_ticket");
    let t_submit = parse_biguint_hex(&fixture.inputs.t_submit).expect("T_submit");
    let t_share = parse_biguint_hex(&fixture.inputs.t_share).expect("T_share");

    let ticket_result = ticket(&c, &pk, &n, &t_ticket);
    assert_eq!(ticket_result.valid, fixture.expected.ticket.valid);
    assert_eq!(
        ticket_result.hash_bytes.to_hex(),
        fixture.expected.ticket.hash_bytes
    );
    assert_eq!(
        ticket_result.hash_int.to_string(),
        fixture.expected.ticket.hash_int
    );

    let sh = share_hash(&c, &pk, &n, &j, &canon_hash);
    assert_eq!(sh.to_hex(), fixture.expected.share_hash);
    assert_eq!(
        digest_to_biguint(&sh).to_string(),
        fixture.expected.share_hash_int
    );
    assert_eq!(share_score(&sh).to_string(), fixture.expected.share_score);
    assert_eq!(
        difficulty_weight(&t_share)
            .expect("difficulty weight")
            .to_string(),
        fixture.expected.difficulty_weight
    );
    assert_eq!(
        min_share_score(&t_share, 1_000_000_000)
            .expect("min share score")
            .to_string(),
        fixture.expected.min_share_score
    );

    let submit_hash = submission_pow_hash(&c, &pk, &nonce_s, &canon_hash);
    assert_eq!(
        submit_hash.to_hex(),
        fixture.expected.submission_pow.hash_bytes
    );
    let (ok, submit_hash_int) = submission_pow_ok(&c, &pk, &nonce_s, &canon_hash, &t_submit);
    assert_eq!(ok, fixture.expected.submission_pow.ok);
    assert_eq!(
        submit_hash_int.to_string(),
        fixture.expected.submission_pow.hash_int
    );
}
