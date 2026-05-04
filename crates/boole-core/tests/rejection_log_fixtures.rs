use boole_core::{
    json_rejection_line, reason_key, reason_key_typed, rejection_event_from_json,
    rejection_event_json, rejection_event_line, LoggedRejectionReason, RejectionEvent,
    RingRejectionLogger,
};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fixture {
    reason_cases: Vec<ReasonCase>,
    ring_case: RingCase,
    ring_one_case: RingCase,
    capacity_error: CapacityError,
    json_case: JsonCase,
    composite_case: CompositeCase,
}

#[derive(Debug, Deserialize)]
struct ReasonCase {
    reason: Value,
    key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RingCase {
    capacity: usize,
    inputs: Vec<Value>,
    expected_events: Vec<Value>,
    expected_total: usize,
    expected_counts: BTreeMap<String, usize>,
}

#[derive(Debug, Deserialize)]
struct CapacityError {
    ok: bool,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct JsonCase {
    inputs: Vec<Value>,
    output: String,
    lines: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CompositeCase {
    inputs: Vec<Value>,
    ring_events: Vec<Value>,
    json_output: String,
}

#[test]
fn rejection_event_typed_adapter_roundtrips_json_and_line_format() {
    let fixture: Fixture = serde_json::from_str(include_str!(
        "../../../fixtures/protocol/rejection-log/v1.json"
    ))
    .expect("fixture parses");

    let raw = &fixture.ring_case.inputs[2];
    let typed = rejection_event_from_json(raw).expect("typed rejection event");
    assert_eq!(
        typed,
        RejectionEvent {
            ts: 1_800_000_000_002,
            ip: "192.0.2.3".to_string(),
            pk: Some(
                "0303030303030303030303030303030303030303030303030303030303030303".to_string()
            ),
            c: Some("1616161616161616161616161616161616161616161616161616161616161616".to_string()),
            reason: LoggedRejectionReason::Decode {
                field: "canon".to_string(),
                detail: "invalid hex".to_string(),
            },
        }
    );
    assert_eq!(reason_key_typed(&typed.reason), "decode:canon");
    assert_eq!(rejection_event_json(&typed), *raw);
    assert_eq!(rejection_event_line(&typed), json_rejection_line(raw));

    let mut ring = RingRejectionLogger::new(2).expect("ring");
    ring.record_typed(typed.clone());
    assert_eq!(ring.events_typed(), vec![typed]);
    assert_eq!(ring.events(), vec![raw.clone()]);
}

#[test]
fn rejection_log_matches_typescript_golden_fixture() {
    let fixture: Fixture = serde_json::from_str(include_str!(
        "../../../fixtures/protocol/rejection-log/v1.json"
    ))
    .expect("fixture parses");

    for case in &fixture.reason_cases {
        assert_eq!(reason_key(&case.reason), case.key);
    }

    assert_ring_case(&fixture.ring_case);
    assert_ring_case(&fixture.ring_one_case);

    let capacity = RingRejectionLogger::new(0);
    assert_eq!(capacity.is_ok(), fixture.capacity_error.ok);
    assert_eq!(
        capacity.expect_err("expected capacity error"),
        fixture.capacity_error.error.unwrap()
    );

    assert_eq!(
        json_lines(&fixture.json_case.inputs).join("\n") + "\n",
        fixture.json_case.output
    );
    assert_eq!(
        json_lines(&fixture.json_case.inputs),
        fixture.json_case.lines
    );

    let mut composite_ring = RingRejectionLogger::new(8).expect("ring");
    for event in &fixture.composite_case.inputs {
        composite_ring.record(event.clone());
    }
    assert_eq!(composite_ring.events(), fixture.composite_case.ring_events);
    assert_eq!(
        json_lines(&fixture.composite_case.inputs).join("\n") + "\n",
        fixture.composite_case.json_output
    );
}

fn assert_ring_case(case: &RingCase) {
    let mut ring = RingRejectionLogger::new(case.capacity).expect("ring");
    for event in &case.inputs {
        ring.record(event.clone());
    }
    assert_eq!(ring.events(), case.expected_events);
    assert_eq!(ring.total_count(), case.expected_total);
    assert_eq!(ring.counts_by_reason(), case.expected_counts);
}

fn json_lines(events: &[Value]) -> Vec<String> {
    events.iter().map(json_rejection_line).collect()
}
