//! Read-only public projection of the installed, signature-verified task.
//! Never reads model-selected files, issues grants, or returns a stored receipt.
use super::*;

pub(super) fn router<L>(service: Arc<ClosedLocalReplayService<CanaryReplayAuthority, L>>) -> Router
where
    L: LauncherTransport<ExecutionRequest>,
{
    Router::new()
        .route("/native-shadow/problem", get(problem::<L>))
        .route("/native-shadow/status", get(status::<L>))
        .with_state(service)
}

async fn status<L>(
    State(service): State<Arc<ClosedLocalReplayService<CanaryReplayAuthority, L>>>,
) -> Response
where
    L: LauncherTransport<ExecutionRequest>,
{
    let grant = &service.replay_authority.grant;
    let budget = service
        .replay_authority
        .budget
        .lock()
        .ok()
        .and_then(|budget| budget.status(grant).ok());
    let ready = !service.poisoned.load(Ordering::Acquire) && budget.is_some();
    let candidate_bound = budget.as_ref().map(|b| b.candidate_bound);
    let submission_allowed = ready && candidate_bound == Some(false);
    json_response(
        if ready {
            StatusCode::OK
        } else {
            StatusCode::SERVICE_UNAVAILABLE
        },
        json!({
            "schema": "boole.development.verifier-status.v1",
            "serviceReady": ready,
            "submissionAllowed": submission_allowed,
            "taskState": if !ready { "unavailable" } else if submission_allowed { "unused" } else { "consumed" },
            "candidateBound": candidate_bound,
            "checkerExecutionReserved": budget.as_ref().map(|b| b.execution_reserved),
            "submissionIdentity": submission_identity(grant),
            "maxCandidates": 1,
            "maxCheckerExecutions": 1,
            "redeliveryRequiresOperatorAuthorization": true,
            "loopbackOnly": true,
            "nonIssuable": true,
            "mineableNow": false,
            "activationAllowed": false
        }),
    )
}

async fn problem<L>(
    State(service): State<Arc<ClosedLocalReplayService<CanaryReplayAuthority, L>>>,
) -> Response
where
    L: LauncherTransport<ExecutionRequest>,
{
    let grant = &service.replay_authority.grant;
    let Ok(task) = serde_json::from_slice::<Value>(grant.task_bytes()) else {
        return json_response(
            StatusCode::SERVICE_UNAVAILABLE,
            json!({"error": "public-task-unavailable"}),
        );
    };
    let Ok(anchor) = std::str::from_utf8(grant.anchor_bytes()) else {
        return json_response(
            StatusCode::SERVICE_UNAVAILABLE,
            json!({"error": "public-task-unavailable"}),
        );
    };
    json_response(
        StatusCode::OK,
        json!({
            "schema": "boole.development.public-problem.v1",
            "globalContract": "Complete only the region between // <<< ACFR-PATCH-BEGIN >>> and // <<< ACFR-PATCH-END >>>. Preserve every byte outside that region, including the function signature. Do not redefine the anchor or add modules, files or dependencies. No unsafe, FFI, inline assembly, raw pointers, transmute, include!, procedural macros, build scripts, or environment/filesystem/network/process/clock/random access.",
            "family": grant.bindings().get("familyVersion"),
            "officialSurface": {"anchorSource": anchor, "helpers": []},
            "outputContract": "Return ACTION: FINAL followed by one rust code fence containing the complete module. No COMPILE or PUBLIC-TEST action is available. One candidate and at most one checker execution are permitted. Read status before submitting. Do not automatically retry or redeliver an uncertain submission; redelivery requires separate operator authorization.",
            "submissionIdentity": submission_identity(grant),
            "task": {
                "description": "Implement acfr_solve. Start acc at a0. For each item, convert each field using Rust as i64 (bool is 0 or 1), multiply by its coefficient, and sum to projection. Update acc = acc * mul + projection. All sums and products use wrapping i64 arithmetic. Process items left to right and return acc; empty input returns a0.",
                "constants": task["constants"],
                "fieldTypes": task["anchor"]["fieldTypes"],
                "edition": task["edition"],
                "scaffold": task["scaffold"]
            },
            "nonIssuable": true,
            "activationAllowed": false
        }),
    )
}

fn submission_identity(grant: &VerifiedCanaryGrant) -> Value {
    json!({
        "schema": "boole.native-shadow.submission.v1",
        "familyVersion": grant.bindings().get("familyVersion"),
        "templateId": grant.bindings().get("templateId"),
        "challengeSha256": grant.bindings().get("challengeSha256"),
        "epoch": grant.epoch()
    })
}
