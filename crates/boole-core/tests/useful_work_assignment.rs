//! BF.2 — pre-registration, forced assignment, result commitment (B2+C2+C3+A1).
//!
//! The B2 contract: `task_index = H(domain ‖ epoch_seed ‖ ticket_id) mod
//! eligible_tasks.length`, `assigned_task = eligible_tasks[task_index]` —
//! **task_id is the assignment RESULT, never a hash input**, so there is
//! no miner-chosen variable to grind and the min(N) cherry-pick path is
//! structurally absent. Assignment is per prepaid mock ticket (A1), not
//! per pk. The empty eligible list is a typed outcome, not a panic (C3).

use boole_core::useful_task_registry::{RegistryEntry, UsefulTaskRegistry};
use boole_core::useful_work::{
    assign_task, assign_task_weighted, result_commitment, settle_assignment, AssignmentError,
    AssignmentOutcome, MockTicketLedger, TaskSpecIdentity,
};
use boole_core::Hex32;
use serde_json::{json, Value};

const FIXTURE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/protocol/useful-work/assignment-v0.json"
);

fn digest(byte: u8) -> String {
    hex::encode([byte; 32])
}

fn hex32(byte: u8) -> Hex32 {
    Hex32::from_hex(&digest(byte)).unwrap()
}

fn task(spec_id: &str) -> TaskSpecIdentity {
    TaskSpecIdentity::from_json_value(&json!({
        "specId": spec_id,
        "variantId": "v1",
        "componentId": "full-round",
        "propertyId": "constraint-completeness",
        "specVersion": 1,
        "taskKind": { "kind": "buildNew" }
    }))
    .expect("valid task")
}

/// The frozen BF.1a ordering: task_id ascending.
fn eligible(count: u8) -> Vec<TaskSpecIdentity> {
    let mut tasks: Vec<TaskSpecIdentity> =
        (0..count).map(|i| task(&format!("spec-{i:02}"))).collect();
    tasks.sort_by_key(|t| t.task_id());
    tasks
}

fn ledger_with(tickets: &[Hex32]) -> MockTicketLedger {
    let mut ledger = MockTicketLedger::new();
    for ticket in tickets {
        ledger.issue(*ticket, 0).expect("issue ticket");
    }
    ledger
}

#[test]
fn assignment_is_deterministic_and_task_id_is_the_result() {
    let tasks = eligible(5);
    let ticket = hex32(0x01);
    let ledger = ledger_with(&[ticket]);
    let seed = hex32(0xee);

    let a = assign_task(&seed, &ticket, &tasks, &ledger).expect("assigns");
    let b = assign_task(&seed, &ticket, &tasks, &ledger).expect("assigns again");
    assert_eq!(
        a, b,
        "same (seed, ticket) must re-derive the same assignment"
    );

    let AssignmentOutcome::Assigned {
        task_id,
        task_index,
    } = a
    else {
        panic!("expected an assignment");
    };
    assert_eq!(
        task_id,
        tasks[task_index as usize].task_id(),
        "the assigned task_id is read out of the frozen list at the derived index"
    );
}

#[test]
fn one_ticket_yields_exactly_one_assignment_regardless_of_pk_count() {
    // Sybil pks are free; tickets are not. Assignment takes NO pk input,
    // so a miner with many pks and one ticket still gets one task.
    let tasks = eligible(7);
    let ticket = hex32(0x02);
    let ledger = ledger_with(&[ticket]);
    let seed = hex32(0xee);

    let only = assign_task(&seed, &ticket, &tasks, &ledger).expect("assigns");
    // "Trying different task_ids" has no input to vary: the only lever is
    // another issued ticket, which costs another issuance event.
    let again = assign_task(&seed, &ticket, &tasks, &ledger).expect("assigns");
    assert_eq!(only, again);

    let second_ticket = hex32(0x03);
    let err = assign_task(&seed, &second_ticket, &tasks, &ledger).unwrap_err();
    assert_eq!(
        err.label(),
        "ticket-not-issued",
        "an unissued ticket must not receive an assignment (A1 mock ledger)"
    );
}

#[test]
fn unsorted_eligible_list_is_rejected() {
    // The BF.1a freeze publishes task_id-ascending order; a node feeding a
    // differently-ordered list would derive divergent assignments, so the
    // input contract is enforced, not assumed.
    let mut tasks = eligible(4);
    tasks.reverse();
    let ticket = hex32(0x04);
    let ledger = ledger_with(&[ticket]);
    let err = assign_task(&hex32(0xee), &ticket, &tasks, &ledger).unwrap_err();
    assert_eq!(err.label(), "eligible-list-not-sorted");
}

#[test]
fn empty_eligible_list_is_a_typed_outcome_not_a_panic() {
    // C3: no modulo on an empty list, no error either — the Hash lane
    // keeps producing blocks, so this is a normal outcome for callers.
    let ticket = hex32(0x05);
    let ledger = ledger_with(&[ticket]);
    let outcome = assign_task(&hex32(0xee), &ticket, &[], &ledger).expect("typed outcome");
    assert_eq!(outcome, AssignmentOutcome::NoEligibleTask);
}

#[test]
fn no_eligible_task_leaves_the_ticket_unspent_and_reusable() {
    let ticket = hex32(0x06);
    let mut ledger = ledger_with(&[ticket]);
    let seed = hex32(0xee);

    let outcome = assign_task(&seed, &ticket, &[], &ledger).expect("no eligible task");
    settle_assignment(&mut ledger, &ticket, 0, &outcome).expect("settle no-op");
    assert!(
        !ledger.is_spent(&ticket, 0),
        "C3: a ticket that received no task must not be burned"
    );

    // Next epoch has supply: the SAME ticket is still good.
    let tasks = eligible(3);
    let outcome = assign_task(&hex32(0xef), &ticket, &tasks, &ledger).expect("assigns next epoch");
    assert!(matches!(outcome, AssignmentOutcome::Assigned { .. }));
    settle_assignment(&mut ledger, &ticket, 1, &outcome).expect("spend on assignment");
    assert!(ledger.is_spent(&ticket, 1));

    // A spent ticket cannot settle twice in the same epoch.
    let err = settle_assignment(&mut ledger, &ticket, 1, &outcome).unwrap_err();
    assert_eq!(err.label(), "ticket-already-spent");
}

#[test]
fn duplicate_ticket_issuance_is_rejected() {
    let mut ledger = MockTicketLedger::new();
    ledger.issue(hex32(0x07), 0).expect("first issue");
    let err = ledger.issue(hex32(0x07), 1).unwrap_err();
    assert_eq!(err.label(), "duplicate-ticket-issue");
}

#[test]
fn weighted_assignment_requires_a_valid_pre_seed_weight_table() {
    let tasks = eligible(3);
    let ticket = hex32(0x08);
    let ledger = ledger_with(&[ticket]);
    let seed = hex32(0xee);

    // Deterministic weighted selection with a frozen table.
    let a = assign_task_weighted(&seed, &ticket, &tasks, &[1, 1, 6], &ledger).expect("assigns");
    let b = assign_task_weighted(&seed, &ticket, &tasks, &[1, 1, 6], &ledger).expect("assigns");
    assert_eq!(a, b);

    // Table length must match the frozen list.
    let err = assign_task_weighted(&seed, &ticket, &tasks, &[1, 1], &ledger).unwrap_err();
    assert_eq!(err.label(), "invalid-weight-table");
    // A zero-total table selects nothing deterministically — rejected.
    let err = assign_task_weighted(&seed, &ticket, &tasks, &[0, 0, 0], &ledger).unwrap_err();
    assert_eq!(err.label(), "invalid-weight-table");
}

#[test]
fn late_registration_cannot_enter_the_assignment_input() {
    // Pipeline pin: registry cutoff -> frozen list -> assignment. A task
    // registered after the freeze is rejected upstream (BF.1a), so the
    // assignment input provably predates the seed.
    let authority = hex32(0xaa);
    let mut registry = UsefulTaskRegistry::new(authority);
    let entry = RegistryEntry::from_json_value(&json!({
        "task": task("frozen-spec").to_json_value(),
        "source": { "kind": "protocolKLadder", "ladderId": "k-ladder", "step": 1 },
        "specFidelity": "strictAudited",
        "eligible": true
    }))
    .expect("valid entry");
    registry
        .register(entry, authority)
        .expect("register pre-cutoff");
    registry.freeze().expect("cutoff freeze");

    let late = RegistryEntry::from_json_value(&json!({
        "task": task("late-spec").to_json_value(),
        "source": { "kind": "protocolKLadder", "ladderId": "k-ladder", "step": 2 },
        "specFidelity": "strictAudited",
        "eligible": true
    }))
    .expect("valid entry");
    assert_eq!(
        registry.register(late, authority).unwrap_err().label(),
        "registration-closed"
    );

    let tasks = registry.eligible_tasks().expect("frozen list");
    let ticket = hex32(0x09);
    let ledger = ledger_with(&[ticket]);
    let outcome = assign_task(&hex32(0xee), &ticket, &tasks, &ledger).expect("assigns");
    let AssignmentOutcome::Assigned { task_id, .. } = outcome else {
        panic!("expected assignment");
    };
    assert_eq!(task_id, task("frozen-spec").task_id());
}

#[test]
fn commitment_rejects_every_single_field_swap() {
    let base = result_commitment(&hex32(0x21), 1, 7, &hex32(0x31), &hex32(0x41), &hex32(0x51));
    // Recomputation is deterministic.
    assert_eq!(
        base,
        result_commitment(&hex32(0x21), 1, 7, &hex32(0x31), &hex32(0x41), &hex32(0x51))
    );
    // task_id / spec_version / epoch / reward_pk / submission_id / nonce:
    // swapping any one input must change the commitment (1-bit binding).
    assert_ne!(
        base,
        result_commitment(&hex32(0x22), 1, 7, &hex32(0x31), &hex32(0x41), &hex32(0x51))
    );
    assert_ne!(
        base,
        result_commitment(&hex32(0x21), 2, 7, &hex32(0x31), &hex32(0x41), &hex32(0x51))
    );
    assert_ne!(
        base,
        result_commitment(&hex32(0x21), 1, 8, &hex32(0x31), &hex32(0x41), &hex32(0x51))
    );
    assert_ne!(
        base,
        result_commitment(&hex32(0x21), 1, 7, &hex32(0x32), &hex32(0x41), &hex32(0x51)),
        "reward key swap must break the commitment"
    );
    assert_ne!(
        base,
        result_commitment(&hex32(0x21), 1, 7, &hex32(0x31), &hex32(0x42), &hex32(0x51)),
        "submission_id swap must break the commitment"
    );
    assert_ne!(
        base,
        result_commitment(&hex32(0x21), 1, 7, &hex32(0x31), &hex32(0x41), &hex32(0x52))
    );
}

#[test]
fn golden_assignment_fixture_is_stable() {
    let fixture: Value =
        serde_json::from_str(&std::fs::read_to_string(FIXTURE_PATH).expect("fixture readable"))
            .expect("fixture parses");
    let seed = Hex32::from_hex(fixture["epochSeed"].as_str().expect("epochSeed")).unwrap();
    let tasks: Vec<TaskSpecIdentity> = fixture["eligibleTasks"]
        .as_array()
        .expect("eligibleTasks")
        .iter()
        .map(|value| TaskSpecIdentity::from_json_value(value).expect("fixture task"))
        .collect();
    for case in fixture["assignments"].as_array().expect("assignments") {
        let ticket = Hex32::from_hex(case["ticketId"].as_str().expect("ticketId")).unwrap();
        let ledger = ledger_with(&[ticket]);
        let outcome = assign_task(&seed, &ticket, &tasks, &ledger).expect("assigns");
        let AssignmentOutcome::Assigned {
            task_id,
            task_index,
        } = outcome
        else {
            panic!("fixture expects assignments");
        };
        assert_eq!(
            task_index,
            case["expectedTaskIndex"].as_u64().expect("index")
        );
        assert_eq!(
            task_id.to_hex(),
            case["expectedTaskId"].as_str().expect("task id")
        );
    }
    let c = fixture["commitment"].clone();
    let commitment = result_commitment(
        &Hex32::from_hex(c["taskId"].as_str().unwrap()).unwrap(),
        c["specVersion"].as_u64().unwrap() as u32,
        c["epoch"].as_u64().unwrap(),
        &Hex32::from_hex(c["rewardPk"].as_str().unwrap()).unwrap(),
        &Hex32::from_hex(c["submissionId"].as_str().unwrap()).unwrap(),
        &Hex32::from_hex(c["nonce"].as_str().unwrap()).unwrap(),
    );
    assert_eq!(
        commitment.to_hex(),
        c["expectedCommitment"]
            .as_str()
            .expect("expectedCommitment")
    );
}

/// Regen helper mirroring repo conventions — rewrites the golden fixture
/// in place from the in-code cases.
#[test]
#[ignore = "regen helper: cargo test -p boole-core --test useful_work_assignment regen_assignment_golden_fixture -- --ignored"]
fn regen_assignment_golden_fixture() {
    let seed = hex32(0xee);
    let tasks = eligible(5);
    let tickets = [hex32(0x01), hex32(0x02), hex32(0x03)];
    let assignments: Vec<Value> = tickets
        .iter()
        .map(|ticket| {
            let ledger = ledger_with(&[*ticket]);
            let AssignmentOutcome::Assigned {
                task_id,
                task_index,
            } = assign_task(&seed, ticket, &tasks, &ledger).expect("assigns")
            else {
                panic!("expected assignment");
            };
            json!({
                "ticketId": ticket.to_hex(),
                "expectedTaskIndex": task_index,
                "expectedTaskId": task_id.to_hex(),
            })
        })
        .collect();

    let commitment =
        result_commitment(&hex32(0x21), 1, 7, &hex32(0x31), &hex32(0x41), &hex32(0x51));
    let fixture = json!({
        "domain": "boole.useful-work.assignment.v0",
        "epochSeed": seed.to_hex(),
        "eligibleTasks": tasks.iter().map(|t| t.to_json_value()).collect::<Vec<_>>(),
        "assignments": assignments,
        "commitment": {
            "taskId": hex32(0x21).to_hex(),
            "specVersion": 1,
            "epoch": 7,
            "rewardPk": hex32(0x31).to_hex(),
            "submissionId": hex32(0x41).to_hex(),
            "nonce": hex32(0x51).to_hex(),
            "expectedCommitment": commitment.to_hex(),
        },
    });
    let pretty = format!("{}\n", serde_json::to_string_pretty(&fixture).unwrap());
    std::fs::write(FIXTURE_PATH, pretty).expect("write fixture");
}

#[test]
fn assignment_error_labels_are_stable() {
    assert_eq!(
        AssignmentError::TicketNotIssued.label(),
        "ticket-not-issued"
    );
    assert_eq!(
        AssignmentError::TicketAlreadySpent.label(),
        "ticket-already-spent"
    );
    assert_eq!(
        AssignmentError::EligibleListNotSorted.label(),
        "eligible-list-not-sorted"
    );
    assert_eq!(
        AssignmentError::InvalidWeightTable.label(),
        "invalid-weight-table"
    );
    assert_eq!(
        AssignmentError::DuplicateTicketIssue.label(),
        "duplicate-ticket-issue"
    );
}
