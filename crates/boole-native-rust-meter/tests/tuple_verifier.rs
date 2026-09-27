use boole_native_rust_meter::tuple_verifier::{
    verify, InputError, Verdict, MAX_ANSWER_BYTES, MAX_TASK_BYTES,
};

const SPEC: &[u8] = include_bytes!("../../../fixtures/native-metered-tuple-v1/task.json");
const ANSWER: &[u8] = include_bytes!("../../../fixtures/native-metered-tuple-v1/answer.rs");

fn json(result: &boole_native_rust_meter::tuple_verifier::Verification) -> serde_json::Value {
    serde_json::from_slice(&result.canonical_bytes()).unwrap()
}

#[test]
fn actual_tuple_answer_is_checked_under_the_fixed_meter_policy() {
    let result = verify(SPEC, ANSWER).unwrap();
    assert_eq!(result.verdict(), Verdict::Accepted);
    assert_eq!(result.resource_use().unwrap().prefix_items(), 2080);
    assert_eq!(
        result.canonical_bytes(),
        verify(SPEC, ANSWER).unwrap().canonical_bytes()
    );
}

#[test]
fn oversized_answers_are_refused_before_producing_bound_evidence() {
    assert!(matches!(
        verify(SPEC, &vec![b' '; MAX_ANSWER_BYTES + 1]),
        Err(InputError::AnswerTooLarge)
    ));
}

#[test]
fn wrong_answer_and_task_substitution_fail_the_real_recurrence_check() {
    let wrong = String::from_utf8(ANSWER.to_vec())
        .unwrap()
        .replace("wrapping_mul(3)", "wrapping_mul(4)");
    let original = verify(SPEC, ANSWER).unwrap();
    let result = verify(SPEC, wrong.as_bytes()).unwrap();
    assert_eq!(result.verdict(), Verdict::DeterministicReject);
    assert_eq!(result.reason(), "answer_mismatch");
    assert_ne!(
        json(&result)["answerDigest"],
        json(&original)["answerDigest"]
    );
    assert_ne!(
        json(&result)["corpusDigest"],
        json(&original)["corpusDigest"]
    );

    let mut spec: serde_json::Value = serde_json::from_slice(SPEC).unwrap();
    spec["coeffs"][0] = 4.into();
    let changed = verify(&serde_json::to_vec(&spec).unwrap(), ANSWER).unwrap();
    assert_eq!(changed.reason(), "answer_mismatch");
    assert_ne!(json(&changed)["taskDigest"], json(&original)["taskDigest"]);
}

#[test]
fn task_whitespace_is_not_identity_but_exact_answer_bytes_are() {
    let value: serde_json::Value = serde_json::from_slice(SPEC).unwrap();
    let compact = serde_json::to_vec(&value).unwrap();
    let first = verify(SPEC, ANSWER).unwrap();
    assert_eq!(
        first.canonical_bytes(),
        verify(&compact, ANSWER).unwrap().canonical_bytes()
    );
    let mut whitespace = ANSWER.to_vec();
    whitespace.push(b' ');
    let second = verify(SPEC, &whitespace).unwrap();
    assert_eq!(second.verdict(), Verdict::Accepted);
    assert_ne!(json(&first)["answerDigest"], json(&second)["answerDigest"]);
    assert_ne!(json(&first)["corpusDigest"], json(&second)["corpusDigest"]);
}

#[test]
fn malformed_or_budget_override_tasks_never_produce_a_verdict() {
    let task: serde_json::Value = serde_json::from_slice(SPEC).unwrap();
    for (field, value) in [
        ("maxFuel", serde_json::json!(u64::MAX)),
        (
            "schema",
            serde_json::json!("boole.native-shadow.rust-tuple-task.v1"),
        ),
        ("a0", serde_json::json!(true)),
        ("a0", serde_json::json!(1.5)),
        ("mul", serde_json::json!(1_000_001)),
        ("fieldTypes", serde_json::json!([])),
        ("fieldTypes", serde_json::json!(["usize", "bool"])),
        ("coeffs", serde_json::json!([3])),
        ("taskSeed", serde_json::json!("F".repeat(64))),
    ] {
        let mut changed = task.clone();
        changed[field] = value;
        assert!(
            matches!(
                verify(&serde_json::to_vec(&changed).unwrap(), ANSWER),
                Err(InputError::InvalidTaskSpecification)
            ),
            "{field}"
        );
    }
    let duplicate = String::from_utf8(SPEC.to_vec())
        .unwrap()
        .replacen('{', "{\"a0\":7,", 1);
    assert!(matches!(
        verify(duplicate.as_bytes(), ANSWER),
        Err(InputError::InvalidTaskSpecification)
    ));
    assert!(matches!(
        verify(&vec![b' '; MAX_TASK_BYTES + 1], ANSWER),
        Err(InputError::TaskTooLarge)
    ));
}

#[test]
fn runtime_budget_exhaustion_is_repeatable_and_cannot_be_source_overridden() {
    let heavy = format!(
        "let mut acc: i64 = 0; for it in items {{ {} }} acc",
        "acc = acc.wrapping_add(0);".repeat(20)
    );
    let result = verify(SPEC, heavy.as_bytes()).unwrap();
    assert_eq!(result.verdict(), Verdict::DeterministicReject);
    assert_eq!(result.reason(), "budget_exceeded:fuel");
    assert!(
        result.resource_use().is_none(),
        "no misleading zero or partial counters"
    );
    assert_eq!(
        result.canonical_bytes(),
        verify(SPEC, heavy.as_bytes()).unwrap().canonical_bytes()
    );
    for source in [
        b"loop {}".as_slice(),
        b"std::process::exit(0)",
        b"// maxFuel=0\n7",
        b"unsafe { 7 }",
    ] {
        let rejection = verify(SPEC, source).unwrap();
        assert_eq!(rejection.verdict(), Verdict::DeterministicReject);
        assert_ne!(rejection.reason(), "answer_mismatch");
    }
}

#[test]
fn all_supported_tuple_field_types_keep_wrapping_i64_semantics() {
    for field_type in ["u8", "u16", "u32", "u64", "i8", "i16", "i32", "i64", "bool"] {
        let mut task: serde_json::Value = serde_json::from_slice(SPEC).unwrap();
        task["fieldTypes"] = serde_json::json!([field_type]);
        task["coeffs"] = serde_json::json!([-999_999]);
        task["a0"] = serde_json::json!(1_000_000);
        task["mul"] = serde_json::json!(-1_000_000);
        let value = if field_type == "bool" {
            "if it.0 { 1 } else { 0 }"
        } else {
            "it.0 as i64"
        };
        let source = format!("let mut acc: i64 = 1000000; for it in items {{ let v = {value}; acc = acc.wrapping_mul(-1000000).wrapping_add(v.wrapping_mul(-999999)); }} acc");
        let result = verify(&serde_json::to_vec(&task).unwrap(), source.as_bytes()).unwrap();
        assert_eq!(
            result.verdict(),
            Verdict::Accepted,
            "{field_type}: {}",
            result.reason()
        );
        assert_eq!(json(&result)["nonIssuable"], true);
        assert_eq!(json(&result)["activationAllowed"], false);
    }
}

#[test]
fn cross_platform_verdict_corpus_matches_independent_golden_bytes() {
    let corpus: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../../fixtures/native-metered-tuple-v1/corpus.json"
    ))
    .unwrap();
    for case in corpus["cases"].as_array().unwrap() {
        let answer = hex::decode(case["answerHex"].as_str().unwrap()).unwrap();
        let result = verify(SPEC, &answer).unwrap();
        assert_eq!(
            result.canonical_bytes(),
            serde_json::to_vec(&case["expected"]).unwrap(),
            "{}",
            case["name"]
        );
    }
}
