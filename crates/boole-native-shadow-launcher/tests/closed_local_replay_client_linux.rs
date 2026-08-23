#[cfg(target_os = "linux")]
use std::net::Shutdown;
#[cfg(target_os = "linux")]
use std::os::unix::net::UnixStream;
#[cfg(target_os = "linux")]
use std::time::Duration;

#[cfg(target_os = "linux")]
use boole_native_shadow_protocol::{
    encode_execution_request_frame,
    installed_authority::open_verified_installed_closed_local_replay_execution_authorities,
    read_closed_local_replay_execution_ready, read_execution_report, sha256_hex,
    validate_closed_local_replay_execution_session, write_execution_hello, write_execution_request,
    CheckerReason, CheckerVerdict, ClosedLocalReplayPreIntakeFields,
    ClosedLocalReplayPreIntakeReason, ClosedLocalReplaySubmissionFields, ExecutionHello,
    ExecutionRequest,
};

#[cfg(target_os = "linux")]
const SOCKET_PATH: &str = "/run/boole/native-shadow/launcher.sock";
#[cfg(target_os = "linux")]
const FAMILY_VERSION: &str = "TUPLE-STRUCT-PROJECT/RUST-TUPLE-STRUCT-PROJECT-V1";
#[cfg(target_os = "linux")]
const TEMPLATE_ID: &str = "800eee9c303c6a0e771e3a3db914eb15ea4ca68d10b19385d60fedd2c23e04b5";
#[cfg(target_os = "linux")]
const CHALLENGE_SHA256: &str = "0b32a406d00a858545b98c0d0937fd940dcfc368fe8a7ef171acc2159fa0f4c1";
#[cfg(target_os = "linux")]
const SESSION_TIMEOUT: Duration = Duration::from_secs(120);

#[cfg(target_os = "linux")]
const ACCEPTED_RAW: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/native-shadow/a-rooted-native-mining-e2e-v1-real-history/replay-accepted.raw.txt"
));
#[cfg(target_os = "linux")]
const TAMPERED_RAW: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/native-shadow/a-rooted-native-mining-e2e-v1-real-history/replay-tampered.raw.txt"
));
#[cfg(target_os = "linux")]
const CONSTANT_RAW: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/native-shadow/a-rooted-native-mining-e2e-v1-real-history/replay-constant.raw.txt"
));
#[cfg(target_os = "linux")]
const EMPTY_RAW: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/native-shadow/a-rooted-native-mining-e2e-v1-real-history/replay-empty.raw.txt"
));
#[cfg(target_os = "linux")]
const ACCEPTED_SOURCE_WITH_LF: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/native-shadow/a-rooted-native-mining-e2e-v1-real-history/accepted.rs"
));
#[cfg(target_os = "linux")]
const TAMPERED_SOURCE_WITH_LF: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/native-shadow/a-rooted-native-mining-e2e-v1-real-history/tampered.rs"
));
#[cfg(target_os = "linux")]
const CONSTANT_SOURCE_WITH_LF: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/native-shadow/a-rooted-native-mining-e2e-v1-real-history/constant.rs"
));

#[cfg(target_os = "linux")]
struct ReplayCase {
    id: &'static str,
    epoch: u64,
    raw_answer: &'static [u8],
    source_with_lf: &'static [u8],
    verdict: CheckerVerdict,
    reason: CheckerReason,
}

fn main() {
    #[cfg(target_os = "linux")]
    if let Err(error) = run_linux() {
        eprintln!("native-shadow closed-local replay client failed: {error}");
        std::process::exit(1);
    }
    #[cfg(not(target_os = "linux"))]
    {
        eprintln!("native-shadow closed-local replay client requires Linux");
        std::process::exit(1);
    }
}

#[cfg(target_os = "linux")]
fn run_linux() -> Result<(), String> {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    let diagnostic = match arguments.as_slice() {
        [] => false,
        [argument] if argument == "--diagnostic-accepted" => true,
        _ => return Err(
            "usage: boole-native-shadow-closed-local-replay-client-linux [--diagnostic-accepted]"
                .to_string(),
        ),
    };
    println!(
        "native-shadow-closed-local-replay-client-pid:{}",
        std::process::id()
    );
    let authorities = open_verified_installed_closed_local_replay_execution_authorities()
        .map_err(|error| format!("open exact installed replay authority failed: {error}"))?;
    let grant = authorities.grant();

    let empty = grant
        .authorize_pre_intake_case(ClosedLocalReplayPreIntakeFields {
            family_version: FAMILY_VERSION,
            template_id: TEMPLATE_ID,
            challenge_sha256: CHALLENGE_SHA256,
            epoch: 3,
            candidate_digest_hex: &sha256_hex(EMPTY_RAW),
            reason: ClosedLocalReplayPreIntakeReason::EmptyResponse,
        })
        .map_err(|error| format!("empty pre-intake row did not stop before the socket: {error}"))?;
    if empty.case_id() != "empty" || empty.max_checker_executions() != 0 {
        return Err("empty replay row unexpectedly acquired checker authority".to_string());
    }

    let cases = [
        ReplayCase {
            id: "accepted",
            epoch: 0,
            raw_answer: ACCEPTED_RAW,
            source_with_lf: ACCEPTED_SOURCE_WITH_LF,
            verdict: CheckerVerdict::Accepted,
            reason: CheckerReason::Accepted,
        },
        ReplayCase {
            id: "tampered",
            epoch: 1,
            raw_answer: TAMPERED_RAW,
            source_with_lf: TAMPERED_SOURCE_WITH_LF,
            verdict: CheckerVerdict::DeterministicReject,
            reason: CheckerReason::CompileOrHiddenTestFailed,
        },
        ReplayCase {
            id: "constant",
            epoch: 2,
            raw_answer: CONSTANT_RAW,
            source_with_lf: CONSTANT_SOURCE_WITH_LF,
            verdict: CheckerVerdict::DeterministicReject,
            reason: CheckerReason::CompileOrHiddenTestFailed,
        },
    ];

    let selected_cases = if diagnostic { &cases[..1] } else { &cases[..] };
    for (index, replay_case) in selected_cases.iter().enumerate() {
        let source = replay_case
            .source_with_lf
            .strip_suffix(b"\n")
            .ok_or_else(|| format!("{} source lacks its one frozen trailing LF", replay_case.id))?;
        let prepared = grant
            .prepare_execution_case(ClosedLocalReplaySubmissionFields {
                family_version: FAMILY_VERSION,
                template_id: TEMPLATE_ID,
                challenge_sha256: CHALLENGE_SHA256,
                epoch: replay_case.epoch,
                candidate_digest_hex: &sha256_hex(replay_case.raw_answer),
                submission_source_digest_hex: &sha256_hex(source),
            })
            .map_err(|error| format!("prepare {} replay row failed: {error}", replay_case.id))?;
        if prepared.case_id() != replay_case.id {
            return Err(format!(
                "epoch {} selected case {} instead of {}",
                replay_case.epoch,
                prepared.case_id(),
                replay_case.id
            ));
        }
        let nonce_hex = format!("{:02x}", index + 1).repeat(32);
        let request = prepared
            .build_execution_request(&nonce_hex, replay_case.raw_answer, source)
            .map_err(|error| format!("build {} Execute failed: {error}", replay_case.id))?;
        execute_one(replay_case, &request, diagnostic)?;
    }

    if diagnostic {
        println!(
            "native-shadow-containment-layer-diagnostic-client-complete:launcher_connections=1"
        );
    } else {
        println!(
            "native-shadow-closed-local-replay-client-complete:launcher_connections=3:empty_connections=0"
        );
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn execute_one(
    replay_case: &ReplayCase,
    request: &ExecutionRequest,
    diagnostic: bool,
) -> Result<(), String> {
    let request_frame = encode_execution_request_frame(request)
        .map_err(|error| format!("encode {} Execute failed: {error}", replay_case.id))?;
    let hello = ExecutionHello::try_from_execution_request_frame(&request_frame)
        .map_err(|error| format!("derive {} Hello failed: {error}", replay_case.id))?;
    let mut stream = UnixStream::connect(SOCKET_PATH)
        .map_err(|error| format!("connect {} replay session failed: {error}", replay_case.id))?;
    stream
        .set_read_timeout(Some(SESSION_TIMEOUT))
        .map_err(|error| format!("set {} read deadline failed: {error}", replay_case.id))?;
    stream
        .set_write_timeout(Some(SESSION_TIMEOUT))
        .map_err(|error| format!("set {} write deadline failed: {error}", replay_case.id))?;

    write_execution_hello(&mut stream, &hello)
        .map_err(|error| format!("write {} Hello failed: {error}", replay_case.id))?;
    let ready = read_closed_local_replay_execution_ready(&mut stream)
        .map_err(|error| format!("read {} Ready-v3 failed: {error}", replay_case.id))?
        .ok_or_else(|| format!("launcher closed before {} Ready-v3", replay_case.id))?;
    write_execution_request(&mut stream, request)
        .map_err(|error| format!("write {} Execute failed: {error}", replay_case.id))?;
    stream
        .shutdown(Shutdown::Write)
        .map_err(|error| format!("close {} client write half failed: {error}", replay_case.id))?;
    let report = read_execution_report(&mut stream)
        .map_err(|error| format!("read {} Report failed: {error}", replay_case.id))?
        .ok_or_else(|| format!("launcher closed before {} Report", replay_case.id))?;
    if read_execution_report(&mut stream)
        .map_err(|error| format!("read {} terminal EOF failed: {error}", replay_case.id))?
        .is_some()
    {
        return Err(format!("launcher sent a second {} Report", replay_case.id));
    }
    let validated =
        validate_closed_local_replay_execution_session(&hello, &ready, &request_frame, &report)
            .map_err(|error| {
                format!("validate exact {} session failed: {error}", replay_case.id)
            })?;
    if diagnostic {
        let value = serde_json::to_value(&report)
            .map_err(|error| format!("serialize safe Report diagnostic failed: {error}"))?;
        let summary = serde_json::json!({
            "waitStatus": value.get("waitStatus"),
            "timedOut": value.get("timedOut"),
            "resourceObservations": value.get("resourceObservations"),
            "checkerResult": value.get("checkerResult"),
            "cleanup": value.get("cleanup"),
        });
        println!(
            "native-shadow-containment-layer-diagnostic-report:verdict={}:reason={}:metadata={}",
            validated
                .checker_verdict()
                .map(verdict_label)
                .unwrap_or("none"),
            validated
                .checker_reason()
                .map(reason_label)
                .unwrap_or("none"),
            serde_json::to_string(&summary)
                .map_err(|error| format!("encode safe Report diagnostic failed: {error}"))?
        );
        return Ok(());
    }
    if validated.checker_verdict() != Some(replay_case.verdict)
        || validated.checker_reason() != Some(replay_case.reason)
        || !validated.cleanup_complete()
    {
        return Err(format!(
            "{} exact Report differs: verdict={:?}, reason={:?}, cleanup={}",
            replay_case.id,
            validated.checker_verdict(),
            validated.checker_reason(),
            validated.cleanup_complete()
        ));
    }
    println!(
        "native-shadow-closed-local-replay-report:{}:{}:{}:cleanup=true",
        replay_case.id,
        verdict_label(replay_case.verdict),
        reason_label(replay_case.reason)
    );
    Ok(())
}

#[cfg(target_os = "linux")]
fn verdict_label(verdict: CheckerVerdict) -> &'static str {
    match verdict {
        CheckerVerdict::Accepted => "accepted",
        CheckerVerdict::DeterministicReject => "deterministic_reject",
        CheckerVerdict::RetryableUnavailable => "retryable_unavailable",
    }
}

#[cfg(target_os = "linux")]
fn reason_label(reason: CheckerReason) -> &'static str {
    match reason {
        CheckerReason::Accepted => "accepted",
        CheckerReason::CompileOrHiddenTestFailed => "compile_or_hidden_test_failed",
        _ => "unexpected_reason",
    }
}
