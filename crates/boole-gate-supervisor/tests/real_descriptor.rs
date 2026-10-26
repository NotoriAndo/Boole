use boole_gate_supervisor::{
    codec::digest,
    real_descriptor::{hash, Descriptor},
};
#[test]
fn python_descriptor_shared_acceptance_and_refusal_vectors() {
    let cases: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/m3-real-gate/descriptor-cases.json"
    ))
    .unwrap();
    let mut mismatches = vec![];
    for case in cases.as_array().unwrap() {
        let raw = hex::decode(case["raw_hex"].as_str().unwrap()).unwrap();
        let decoded = Descriptor::decode(&raw);
        if decoded.is_ok() != case["valid"].as_bool().unwrap() {
            mismatches.push(case["name"].clone());
        }
        if let (Ok(d), Some(identity)) = (decoded, case["identity"].as_str()) {
            assert_eq!(d.digest(), hash(identity).unwrap());
            assert_ne!(d.digest(), digest(&raw));
        }
    }
    assert!(
        mismatches.is_empty(),
        "Rust/Python descriptor disagreements: {mismatches:?}"
    );
}
