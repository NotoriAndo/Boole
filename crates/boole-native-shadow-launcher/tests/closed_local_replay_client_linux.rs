#[cfg(target_os = "linux")]
use std::mem::{self, MaybeUninit};
#[cfg(target_os = "linux")]
use std::net::Shutdown;
#[cfg(target_os = "linux")]
use std::os::fd::AsRawFd;
#[cfg(target_os = "linux")]
use std::os::unix::net::UnixStream;
#[cfg(target_os = "linux")]
use std::time::Duration;

#[cfg(target_os = "linux")]
use boole_native_shadow_protocol::{
    encode_execution_request_frame,
    installed_authority::{
        open_verified_installed_authority_bundle,
        open_verified_installed_closed_local_replay_execution_authorities,
    },
    read_closed_local_replay_execution_ready, read_execution_report, read_qualification_ready,
    resolve_fixed_service_identities, sha256_hex, validate_closed_local_replay_execution_session,
    write_execution_hello, write_execution_request, write_qualification_hello, CheckerReason,
    CheckerVerdict, ClosedLocalReplayPreIntakeFields, ClosedLocalReplayPreIntakeReason,
    ClosedLocalReplaySubmissionFields, ExecutionHello, ExecutionRequest, QualificationHello,
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

#[cfg(target_os = "linux")]
struct QualifiedLauncher {
    launcher_pid: u32,
    launcher_instance_id_hex: String,
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
        [argument] if argument == "--qualified-all-three" => false,
        [argument] if argument == "--diagnostic-accepted" => true,
        _ => return Err(
            "usage: boole-native-shadow-closed-local-replay-client-linux (--qualified-all-three|--diagnostic-accepted)".to_string(),
        ),
    };
    println!(
        "native-shadow-closed-local-replay-client-pid:{}",
        std::process::id()
    );
    let authorities = open_verified_installed_closed_local_replay_execution_authorities()
        .map_err(|error| format!("open exact installed replay authority failed: {error}"))?;
    let grant = authorities.grant();
    let qualified = if diagnostic {
        None
    } else {
        Some(qualify_launcher()?)
    };

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
        execute_one(replay_case, &request, diagnostic, qualified.as_ref())?;
    }

    if diagnostic {
        println!(
            "native-shadow-containment-layer-diagnostic-client-complete:launcher_connections=1"
        );
    } else {
        println!(
            "native-shadow-closed-local-replay-client-complete:launcher_connections=4:qualification_connections=1:checker_connections=3:empty_connections=0"
        );
    }
    Ok(())
}

#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
fn qualify_launcher() -> Result<QualifiedLauncher, String> {
    let authority = open_verified_installed_authority_bundle()
        .map_err(|error| format!("open installed qualification authority failed: {error}"))?;
    let identities = resolve_fixed_service_identities()
        .map_err(|error| format!("resolve fixed service identities failed: {error}"))?;
    let mut nonce = [0_u8; 32];
    // SAFETY: nonce is writable storage of exactly 32 bytes and flags=0 has
    // no pointer or lifetime side effects beyond this one kernel call.
    let nonce_bytes = unsafe { libc::getrandom(nonce.as_mut_ptr().cast(), nonce.len(), 0) };
    if nonce_bytes != nonce.len() as isize {
        return Err(format!(
            "qualification getrandom returned {nonce_bytes} bytes instead of 32"
        ));
    }
    let nonce_hex = hex::encode(nonce);
    let hello = QualificationHello::try_new(
        nonce_hex.clone(),
        authority.execution_policy_digest().to_string(),
        authority.toolchain_identity_digest().to_string(),
        authority.registry_digest().to_string(),
    )
    .map_err(|error| format!("build qualification Hello failed: {error}"))?;
    let mut stream = UnixStream::connect(SOCKET_PATH)
        .map_err(|error| format!("connect qualification session failed: {error}"))?;
    stream
        .set_read_timeout(Some(SESSION_TIMEOUT))
        .map_err(|error| format!("set qualification read deadline failed: {error}"))?;
    stream
        .set_write_timeout(Some(SESSION_TIMEOUT))
        .map_err(|error| format!("set qualification write deadline failed: {error}"))?;
    let launcher_peer = launcher_peer_credentials(&stream)?;
    if launcher_peer.0 == 0 || launcher_peer.1 != 0 || launcher_peer.2 != 0 {
        return Err("qualification socket peer is not the root launcher".to_string());
    }
    write_qualification_hello(&mut stream, &hello)
        .map_err(|error| format!("write qualification Hello failed: {error}"))?;
    let ready = read_qualification_ready(&mut stream)
        .map_err(|error| format!("read qualification Ready failed: {error}"))?
        .ok_or_else(|| "launcher closed before qualification Ready".to_string())?;
    if ready.nonce_hex() != nonce_hex
        || ready.registry_digest_hex() != authority.registry_digest()
        || ready.execution_policy_digest_hex() != authority.execution_policy_digest()
        || ready.toolchain_identity_digest_hex() != authority.toolchain_identity_digest()
        || ready.launcher_pid() != launcher_peer.0
        || ready.launcher_uid() != launcher_peer.1
        || ready.launcher_gid() != launcher_peer.2
        || ready.node_uid() != identities.node_uid()
        || ready.node_gid() != identities.node_gid()
        || ready.checker_uid() != identities.checker_uid()
        || ready.checker_gid() != identities.checker_gid()
        || !ready.startup_recovery_complete()
        || ready.active_execution_leaves() != 0
        || ready.unexpected_direct_cgroup_children() != 0
        || !ready.manager_subgroup_verified()
        || ready.activation_allowed()
        || !ready.ready()
    {
        return Err("qualification Ready does not match the exact launcher authority".to_string());
    }
    stream
        .shutdown(Shutdown::Write)
        .map_err(|error| format!("close qualification client write half failed: {error}"))?;
    if read_qualification_ready(&mut stream)
        .map_err(|error| format!("read qualification terminal EOF failed: {error}"))?
        .is_some()
    {
        return Err("launcher sent a second qualification Ready".to_string());
    }
    Ok(QualifiedLauncher {
        launcher_pid: ready.launcher_pid(),
        launcher_instance_id_hex: ready.launcher_instance_id_hex().to_string(),
    })
}

#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
fn launcher_peer_credentials(stream: &UnixStream) -> Result<(u32, u32, u32), String> {
    let mut credentials = MaybeUninit::<libc::ucred>::uninit();
    let mut length = mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: credentials is exact writable storage and stream owns one live
    // Unix socket descriptor for the duration of getsockopt.
    let status = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            credentials.as_mut_ptr().cast::<libc::c_void>(),
            &mut length,
        )
    };
    if status != 0 || length as usize != mem::size_of::<libc::ucred>() {
        return Err(format!(
            "read qualification launcher SO_PEERCRED failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    // SAFETY: successful getsockopt with the exact expected length initialized
    // every byte of libc::ucred.
    let credentials = unsafe { credentials.assume_init() };
    let pid = u32::try_from(credentials.pid)
        .map_err(|_| "qualification launcher SO_PEERCRED PID is negative".to_string())?;
    Ok((pid, credentials.uid, credentials.gid))
}

#[cfg(target_os = "linux")]
fn execute_one(
    replay_case: &ReplayCase,
    request: &ExecutionRequest,
    diagnostic: bool,
    qualified: Option<&QualifiedLauncher>,
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
    if let Some(qualified) = qualified {
        if ready.launcher_pid() != qualified.launcher_pid
            || ready.launcher_instance_id_hex() != qualified.launcher_instance_id_hex.as_str()
        {
            return Err(format!(
                "{} execution Ready does not match the qualified launcher instance",
                replay_case.id
            ));
        }
    }
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
