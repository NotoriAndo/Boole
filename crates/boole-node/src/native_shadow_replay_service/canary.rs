use super::*;
use boole_native_shadow_protocol::{
    fresh_answer_canary::{CanaryBudget, VerifiedCanaryGrant},
    installed_authority::InstalledCanaryRedeliveryAuthority,
    ExecutionRequest,
};

struct CanaryReplayAuthority {
    grant: VerifiedCanaryGrant,
    budget: Mutex<CanaryBudget>,
    redelivery: Option<InstalledCanaryRedeliveryAuthority>,
}

/// Development-only Linux service. No runtime flag can turn the existing
/// replay binary into this service and no HTTP request can select its keys.
#[cfg(target_os = "linux")]
pub async fn serve_installed_fresh_answer_canary() -> anyhow::Result<()> {
    use boole_native_shadow_protocol::{
        fresh_answer_canary::CanaryBudgetRole,
        installed_authority::{
            open_installed_canary,
            open_verified_installed_closed_local_replay_execution_authorities,
        },
        resolve_fixed_service_identities,
    };
    let identities = resolve_fixed_service_identities()?;
    // Retain and verify every existing checker, toolchain and containment
    // authority. None of the frozen fixture-answer permissions are spent.
    let installed = open_verified_installed_closed_local_replay_execution_authorities()?;
    let canary = open_installed_canary()?;
    let (budget, directory) = canary.open_budget(
        CanaryBudgetRole::Node,
        identities.node_uid(),
        identities.node_gid(),
    )?;
    let grant = canary.grant();
    let label =
        std::path::Path::new("/var/lib/boole/native-shadow/canary-node").join(grant.journal_id());
    let mut journal_authority = NativeShadowJournalAuthority::open_retained_production_dir(
        directory,
        &label,
        std::ffi::OsStr::new("verdict-v1.ndjson"),
        identities.node_uid(),
        identities.node_gid(),
    )?;
    let policy = crate::native_shadow::NativeShadowExecutionPolicyDigest::try_from(
        grant.bindings().get("executionPolicyDigest"),
    )
    .map_err(anyhow::Error::msg)?;
    let recovery = crate::native_shadow::recover_verified_closed_local_replay_state(
        grant.bindings().get("registryVersion"),
        grant.bindings().get("registryDigest"),
        &policy,
        &mut journal_authority,
    )?;
    recovery
        .attempts
        .validate_against_canary(grant, &budget, &policy)?;
    let readiness = qualify_and_validate_before_recovery_refusal(
        &recovery.stuck_in_flight,
        || Ok(crate::native_shadow_qualification::qualify_installed_native_shadow_launcher()?),
        |ready| {
            anyhow::ensure!(
                ready.registry_digest_hex() == installed.grant().production_registry_digest_hex()
                    && ready.execution_policy_digest_hex() == policy.as_str()
                    && ready.toolchain_identity_digest_hex()
                        == grant.bindings().get("toolchainDigest"),
                "canary qualified launcher material binding"
            );
            Ok(())
        },
    )?;
    let (grant, redelivery) = canary.into_parts();
    let service = Arc::new(ClosedLocalReplayService {
        replay_authority: CanaryReplayAuthority {
            grant,
            budget: Mutex::new(budget),
            redelivery: Some(redelivery),
        },
        launcher: installed_launcher_transport::InstalledLauncherTransport::new(readiness),
        execution_gate: Arc::new(NativeShadowExecutionGate::new()),
        journal: Mutex::new(ReplayJournalState {
            authority: journal_authority,
            store: recovery.store,
            exhaustion: recovery.exhaustion_ledger,
            attempts: recovery.attempts,
        }),
        poisoned: Arc::new(AtomicBool::new(false)),
    });
    let listener = tokio::net::TcpListener::bind(fixed_http_listener_address()).await?;
    axum::serve(listener, build_router(service)).await?;
    Ok(())
}

impl CanaryReplayAuthority {
    fn identity(&self, request: &ExecutionRequest) -> Result<ReplayCaseIdentity, &'static str> {
        Ok(ReplayCaseIdentity {
            four_tuple: NativeShadowFourTuple {
                family_version: request.family_version().to_string(),
                template_id: request.template_id().to_string(),
                challenge_sha256: request.challenge_sha256().to_string(),
                epoch: request.epoch(),
            },
            registry_version: request.registry_version().to_string(),
            registry_digest: request.registry_digest_hex().to_string(),
            execution_policy_digest:
                crate::native_shadow::NativeShadowExecutionPolicyDigest::try_from(
                    request.execution_policy_digest_hex(),
                )
                .map_err(|_| "canary_policy_invalid")?,
            operation_id_hex: request.operation_id_hex().to_string(),
            candidate_digest: request.candidate_digest_hex().to_string(),
            submission_digest: request.submission_digest_hex().to_string(),
            anchor_digest: request.anchor_digest_hex().to_string(),
            intake_version: request.intake_version().to_string(),
            checker_digest: request.checker_artifact_hash_hex().to_string(),
            checker_policy_digest: request.checker_policy_digest_hex().to_string(),
            toolchain_digest: request.toolchain_identity_digest_hex().to_string(),
        })
    }
}

impl ReplayAuthority for CanaryReplayAuthority {
    type CheckerPrepared = ();
    type PreIntakePrepared = NativeShadowSubmission;
    type Request = ExecutionRequest;

    fn admit_candidate(&self, submission: &NativeShadowSubmission) -> Result<(), &'static str> {
        if !self.grant.matches_task(
            submission.family_version(),
            submission.template_id(),
            submission.challenge_sha256(),
            submission.epoch(),
        ) {
            return Err("canary_task_mismatch");
        }
        self.budget
            .lock()
            .map_err(|_| "canary_budget_unavailable")?
            .admit_candidate(
                &self.grant,
                &submission.candidate_digest_hex(),
                &submission.submission_digest_hex(),
            )
            .map_err(|_| "canary_candidate_already_bound_or_budget_unavailable")
    }

    fn authorize_redelivery(
        &self,
        identity: &ReplayRedeliveryIdentity,
    ) -> Result<(), &'static str> {
        let permit = self
            .redelivery
            .as_ref()
            .ok_or("canary_redelivery_not_authorized")?
            .read(&self.grant)
            .map_err(|_| "canary_redelivery_not_authorized")?;
        self.budget
            .lock()
            .map_err(|_| "canary_budget_unavailable")?
            .consume_redelivery(
                &self.grant,
                permit,
                &identity.candidate_digest,
                &identity.submission_digest,
            )
            .map_err(|_| "canary_redelivery_exhausted_or_mismatch")
    }

    fn redelivery_identity(
        &self,
        submission: &NativeShadowSubmission,
    ) -> Result<ReplayRedeliveryIdentity, &'static str> {
        Ok(ReplayRedeliveryIdentity {
            four_tuple: NativeShadowFourTuple {
                family_version: submission.family_version().to_string(),
                template_id: submission.template_id().to_string(),
                challenge_sha256: submission.challenge_sha256().to_string(),
                epoch: submission.epoch(),
            },
            registry_version: self.grant.bindings().get("registryVersion").to_string(),
            registry_digest: self.grant.bindings().get("registryDigest").to_string(),
            execution_policy_digest:
                crate::native_shadow::NativeShadowExecutionPolicyDigest::try_from(
                    self.grant.bindings().get("executionPolicyDigest"),
                )
                .map_err(|_| "canary_policy_invalid")?,
            candidate_digest: submission.candidate_digest_hex(),
            submission_digest: submission.submission_digest_hex(),
        })
    }

    fn prepare_checker(
        &self,
        submission: &NativeShadowSubmission,
        source: &NativeShadowSubmissionSource,
    ) -> Result<PreparedCheckerCase<(), ExecutionRequest>, &'static str> {
        if !self.grant.matches_task(
            submission.family_version(),
            submission.template_id(),
            submission.challenge_sha256(),
            submission.epoch(),
        ) {
            return Err("canary_task_mismatch");
        }
        let request = self
            .grant
            .prepare_request(
                &fresh_nonce_hex().map_err(|_| "nonce_unavailable")?,
                submission.raw_answer_bytes(),
                source.source().as_bytes(),
            )
            .map_err(|_| "canary_request_binding")?;
        let identity = self.identity(&request)?;
        Ok(PreparedCheckerCase {
            prepared: (),
            request,
            identity,
        })
    }

    fn validate_checker_before_reserve(
        &self,
        prepared: &PreparedCheckerCase<(), ExecutionRequest>,
    ) -> Result<(), &'static str> {
        self.grant
            .validate_request(&prepared.request)
            .map_err(|_| "canary_request_binding")?;
        self.budget
            .lock()
            .map_err(|_| "canary_budget_unavailable")?
            .matches_candidate(
                &prepared.identity.candidate_digest,
                &prepared.identity.submission_digest,
            )
            .then_some(())
            .ok_or("canary_candidate_binding")
    }

    fn authorize_checker(
        &self,
        prepared: PreparedCheckerCase<(), ExecutionRequest>,
    ) -> Result<AuthorizedCheckerCase<ExecutionRequest>, &'static str> {
        let authorization = self
            .budget
            .lock()
            .map_err(|_| "canary_budget_unavailable")?
            .reserve_execution(&self.grant, &prepared.request)
            .map_err(|_| "canary_execution_spent")?;
        let bootstrap =
            VerifiedNativeShadowReplayBootstrap::from_canary_authorization(&authorization)
                .map_err(|_| "canary_bootstrap_binding")?;
        Ok(AuthorizedCheckerCase {
            request: prepared.request,
            identity: prepared.identity,
            bootstrap,
        })
    }

    fn prepare_pre_intake(
        &self,
        submission: &NativeShadowSubmission,
    ) -> Result<PreparedPreIntakeCase<NativeShadowSubmission>, &'static str> {
        let identity = self.redelivery_identity(submission)?;
        Ok(PreparedPreIntakeCase {
            prepared: submission.clone(),
            four_tuple: identity.four_tuple,
            registry_digest: identity.registry_digest,
            execution_policy_digest: identity.execution_policy_digest,
            operation_id_hex: self.grant.operation_id(),
            candidate_digest: identity.candidate_digest,
            submission_digest: identity.submission_digest,
        })
    }

    fn validate_pre_intake_before_reserve(
        &self,
        prepared: &PreparedPreIntakeCase<NativeShadowSubmission>,
    ) -> Result<(), &'static str> {
        self.authorize_pre_intake(prepared.prepared.clone())
    }

    fn authorize_pre_intake(&self, submission: NativeShadowSubmission) -> Result<(), &'static str> {
        if !self.grant.matches_task(
            submission.family_version(),
            submission.template_id(),
            submission.challenge_sha256(),
            submission.epoch(),
        ) {
            return Err("canary_task_mismatch");
        }
        self.budget
            .lock()
            .map_err(|_| "canary_budget_unavailable")?
            .matches_candidate(
                &submission.candidate_digest_hex(),
                &submission.submission_digest_hex(),
            )
            .then_some(())
            .ok_or("canary_candidate_binding")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use boole_native_shadow_protocol::fresh_answer_canary::{
        verify_grant, CanaryBindings, CanaryBudget, CanaryBudgetRole, CanaryGrant, CanaryTrustRoot,
        VerifiedCanaryGrant, SIGNING_DOMAIN,
    };
    use ed25519_dalek::{Signer, SigningKey};
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use tower::ServiceExt;

    fn grant() -> VerifiedCanaryGrant {
        let key = SigningKey::from_bytes(&[47; 32]);
        let bindings = CanaryBindings::installed().unwrap();
        let bytes = serde_json::to_vec(&CanaryGrant::one_task(
            "11".repeat(32),
            "22".repeat(32),
            10,
            bindings.clone(),
        ))
        .unwrap();
        verify_grant(
            &bytes,
            &key.sign(&[SIGNING_DOMAIN, bytes.as_slice()].concat())
                .to_bytes(),
            &CanaryTrustRoot::new(key.verifying_key().to_bytes()).unwrap(),
            &bindings,
        )
        .unwrap()
    }

    struct BoundaryLauncher(std::sync::atomic::AtomicUsize);
    impl LauncherTransport<boole_native_shadow_protocol::ExecutionRequest> for Arc<BoundaryLauncher> {
        fn execute(
            &self,
            request: &boole_native_shadow_protocol::ExecutionRequest,
        ) -> ValidatedLauncherOutcome {
            assert_eq!(request.epoch(), 10);
            self.0.fetch_add(1, Ordering::SeqCst);
            ValidatedLauncherOutcome::Terminal(ValidatedLauncherTerminal {
                verdict: ValidatedLauncherVerdict::Accepted,
                reason_code: "accepted",
            })
        }
    }

    fn request(raw: &str) -> Request<Body> {
        let grant = grant();
        Request::builder()
            .method("POST")
            .uri(SUBMISSION_ROUTE)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::to_vec(&json!({
                    "schema": "boole.native-shadow.submission.v1",
                    "familyVersion": grant.bindings().get("familyVersion"),
                    "templateId": grant.bindings().get("templateId"),
                    "challengeSha256": grant.bindings().get("challengeSha256"),
                    "epoch": grant.epoch(), "rawAnswer": raw,
                }))
                .unwrap(),
            ))
            .unwrap()
    }

    fn service(
        path: &std::path::Path,
        launcher: Arc<BoundaryLauncher>,
    ) -> Arc<ClosedLocalReplayService<CanaryReplayAuthority, Arc<BoundaryLauncher>>> {
        let grant = grant();
        let directory = std::fs::File::open(path).unwrap();
        let metadata = directory.metadata().unwrap();
        let budget = CanaryBudget::open(
            &directory,
            &grant,
            CanaryBudgetRole::Node,
            metadata.uid(),
            metadata.gid(),
        )
        .unwrap();
        let mut authority = NativeShadowJournalAuthority::open_prepared_production(
            path.join("verdict.ndjson"),
            metadata.uid(),
            metadata.gid(),
        )
        .unwrap();
        let policy = crate::native_shadow::NativeShadowExecutionPolicyDigest::try_from(
            grant.bindings().get("executionPolicyDigest"),
        )
        .unwrap();
        let recovery = crate::native_shadow::recover_verified_closed_local_replay_state(
            grant.bindings().get("registryVersion"),
            grant.bindings().get("registryDigest"),
            &policy,
            &mut authority,
        )
        .unwrap();
        recovery
            .attempts
            .validate_against_canary(&grant, &budget, &policy)
            .unwrap();
        Arc::new(ClosedLocalReplayService {
            replay_authority: CanaryReplayAuthority {
                grant,
                budget: Mutex::new(budget),
                redelivery: None,
            },
            launcher,
            execution_gate: Arc::new(NativeShadowExecutionGate::new()),
            journal: Mutex::new(ReplayJournalState {
                authority,
                store: recovery.store,
                exhaustion: recovery.exhaustion_ledger,
                attempts: recovery.attempts,
            }),
            poisoned: Arc::new(AtomicBool::new(false)),
        })
    }

    #[tokio::test]
    async fn signed_fresh_candidate_gets_a_receipt_but_second_candidate_cannot_execute_after_restart(
    ) {
        let path = std::env::temp_dir().join(format!(
            "boole-canary-route-{}-{}",
            std::process::id(),
            boole_testkit::rand_suffix()
        ));
        std::fs::create_dir(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        let launcher = Arc::new(BoundaryLauncher(std::sync::atomic::AtomicUsize::new(0)));
        let first = service(&path, launcher.clone());
        let response = build_router(first.clone())
            .oneshot(request("Fresh answer\n```rust\nfn main() {}\n```"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), HTTP_BODY_LIMIT_BYTES)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["outcome"], "accepted");
        assert!(body["receipt"].is_object());
        drop(first);
        let recovered = service(&path, launcher.clone());
        let response = build_router(recovered.clone())
            .oneshot(request("```rust\nfn changed() {}\n```"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert_eq!(launcher.0.load(Ordering::SeqCst), 1);
        drop(recovered);
        std::fs::remove_file(path.join("node-budget-v1.jsonl")).unwrap();
        std::fs::remove_file(path.join("verdict.ndjson")).unwrap();
        std::fs::remove_dir(path).unwrap();
    }

    #[tokio::test]
    async fn intake_rejection_still_consumes_the_only_candidate_across_restart() {
        for raw in ["not a fenced answer", ""] {
            let path = std::env::temp_dir().join(format!(
                "boole-canary-intake-{}-{}",
                std::process::id(),
                boole_testkit::rand_suffix()
            ));
            std::fs::create_dir(&path).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
            let launcher = Arc::new(BoundaryLauncher(std::sync::atomic::AtomicUsize::new(0)));
            let first = service(&path, launcher.clone());
            let response = build_router(first.clone())
                .oneshot(request(raw))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            drop(first);
            let recovered = service(&path, launcher.clone());
            let response = build_router(recovered.clone())
                .oneshot(request("```rust\nfn changed() {}\n```"))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::CONFLICT);
            assert_eq!(launcher.0.load(Ordering::SeqCst), 0);
            drop(recovered);
            std::fs::remove_file(path.join("node-budget-v1.jsonl")).unwrap();
            std::fs::remove_file(path.join("verdict.ndjson")).unwrap();
            std::fs::remove_dir(path).unwrap();
        }
    }
}
