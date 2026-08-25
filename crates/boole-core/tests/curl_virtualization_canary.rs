//! CURL.3-PREP — the frozen acceptance contract of the clean-Mac
//! Team-ID-free virtualization-entitlement canary.
//!
//! These tests pin the canary contract itself, not a canary run: no VM is
//! started, no artifact is downloaded and no Apple identity is used. The
//! central invariant is that evidence produced on a developer machine can
//! never be recorded as a clean-Mac pass, however well the run went.

use boole_core::{
    evaluate_curl_virtualization_canary, CanaryBootInputPin, CanaryBootInputRole, CanaryBootLoader,
    CanaryExecutionMode, CanaryHostSignature, CanaryLifecyclePhase, CanaryPhaseRecord,
    CanaryResidueScan, CleanMachineEvidence, CurlVirtualizationCanaryRejection,
    CurlVirtualizationCanaryReport, HostArchitecture, MacOsVersion, CURL_CANARY_MINIMUM_MACOS,
    CURL_CANARY_REQUIRED_ENTITLEMENT, CURL_PRODUCT_RELEASE_MINIMUM_MACOS,
};

fn digest(seed: char) -> String {
    seed.to_string().repeat(64)
}

fn pinned_boot_inputs() -> Vec<CanaryBootInputPin> {
    vec![
        CanaryBootInputPin::new(
            CanaryBootInputRole::GuestKernel,
            "guest-kernel",
            41_943_040,
            &digest('a'),
        ),
        CanaryBootInputPin::new(
            CanaryBootInputRole::GuestInitrd,
            "guest-initrd",
            12_582_912,
            &digest('b'),
        ),
        CanaryBootInputPin::new(
            CanaryBootInputRole::GuestRootDisk,
            "guest-root-disk",
            536_870_912,
            &digest('c'),
        ),
    ]
}

fn clean_lifecycle() -> Vec<CanaryPhaseRecord> {
    vec![
        CanaryPhaseRecord::completed(CanaryLifecyclePhase::Boot, CanaryResidueScan::empty()),
        CanaryPhaseRecord::completed(CanaryLifecyclePhase::Shutdown, CanaryResidueScan::empty()),
        CanaryPhaseRecord::completed(CanaryLifecyclePhase::Reboot, CanaryResidueScan::empty()),
    ]
}

/// A report that passes every frozen check. Each test below breaks exactly
/// one element of it, so the rejection it observes is attributable.
fn clean_mac_report() -> CurlVirtualizationCanaryReport {
    CurlVirtualizationCanaryReport {
        macos_version: MacOsVersion::new(14, 0),
        architecture: HostArchitecture::AppleSilicon,
        clean_machine_evidence: CleanMachineEvidence::erased_and_clean(),
        signature: CanaryHostSignature::ad_hoc(&[CURL_CANARY_REQUIRED_ENTITLEMENT]),
        execution_mode: CanaryExecutionMode::EntitledIsolatedVm,
        boot_loader: CanaryBootLoader::VzLinuxBootLoader,
        boot_inputs: pinned_boot_inputs(),
        reboot_boot_inputs: pinned_boot_inputs(),
        lifecycle: clean_lifecycle(),
    }
}

#[test]
fn clean_mac_report_with_every_boundary_clean_passes() {
    let pass = evaluate_curl_virtualization_canary(&clean_mac_report())
        .expect("a clean supported Mac with an entitled ad-hoc run must pass");

    assert_eq!(pass.macos_version(), MacOsVersion::new(14, 0));
    assert_eq!(pass.boot_inputs().len(), 3);
    assert_eq!(
        pass.boot_inputs()[0].role(),
        CanaryBootInputRole::GuestKernel
    );
}

#[test]
fn canary_floor_matches_the_frozen_product_minimum_macos() {
    assert_eq!(
        CURL_CANARY_MINIMUM_MACOS.to_string(),
        CURL_PRODUCT_RELEASE_MINIMUM_MACOS,
        "the canary floor must not drift from the frozen product minimum"
    );
}

#[test]
fn macos_below_the_frozen_floor_is_rejected() {
    let mut report = clean_mac_report();
    report.macos_version = MacOsVersion::new(13, 7);

    let rejection = evaluate_curl_virtualization_canary(&report)
        .expect_err("macOS 13.7 is below the frozen 14.0 floor");

    assert!(
        matches!(
            rejection,
            CurlVirtualizationCanaryRejection::UnsupportedMacOs(_)
        ),
        "unexpected rejection: {rejection}"
    );
    assert!(rejection.to_string().contains("14.0"));
}

#[test]
fn intel_mac_is_rejected() {
    let mut report = clean_mac_report();
    report.architecture = HostArchitecture::Intel;

    let rejection = evaluate_curl_virtualization_canary(&report)
        .expect_err("Intel is outside the frozen Apple Silicon support range");

    assert!(
        matches!(
            rejection,
            CurlVirtualizationCanaryRejection::UnsupportedArchitecture(_)
        ),
        "unexpected rejection: {rejection}"
    );
}

#[test]
fn developer_machine_is_rejected_even_when_every_phase_succeeded() {
    let mut report = clean_mac_report();
    report.clean_machine_evidence = CleanMachineEvidence::developer_machine();

    let rejection = evaluate_curl_virtualization_canary(&report)
        .expect_err("a developer machine can never produce clean-Mac evidence");

    assert!(
        matches!(
            rejection,
            CurlVirtualizationCanaryRejection::MachineNotClean(_)
        ),
        "unexpected rejection: {rejection}"
    );
}

#[test]
fn rejection_names_every_missing_clean_machine_ground() {
    let mut report = clean_mac_report();
    report.clean_machine_evidence = CleanMachineEvidence::new(false, false, false, false);

    let rejection = evaluate_curl_virtualization_canary(&report)
        .expect_err("no clean-machine ground was established");
    let message = rejection.to_string();

    for ground in [
        "erase-install",
        "developer-toolchain-absent",
        "boole-source-tree-absent",
        "prior-boole-install-absent",
    ] {
        assert!(
            message.contains(ground),
            "missing ground {ground} in: {message}"
        );
    }
}

#[test]
fn team_identity_signature_is_rejected_because_it_proves_a_different_path() {
    let mut report = clean_mac_report();
    report.signature =
        CanaryHostSignature::team_identity("ABCDE12345", &[CURL_CANARY_REQUIRED_ENTITLEMENT]);

    let rejection = evaluate_curl_virtualization_canary(&report)
        .expect_err("a Team-ID signature does not exercise the Team-ID-free path");

    assert!(
        matches!(
            rejection,
            CurlVirtualizationCanaryRejection::SigningFormRejected(_)
        ),
        "unexpected rejection: {rejection}"
    );
    assert!(rejection.to_string().contains("ad-hoc"));
}

#[test]
fn unsigned_host_controller_is_rejected() {
    let mut report = clean_mac_report();
    report.signature = CanaryHostSignature::unsigned();

    let rejection = evaluate_curl_virtualization_canary(&report)
        .expect_err("an unsigned controller carries no entitlement");

    assert!(
        matches!(
            rejection,
            CurlVirtualizationCanaryRejection::SigningFormRejected(_)
        ),
        "unexpected rejection: {rejection}"
    );
}

#[test]
fn missing_virtualization_entitlement_is_rejected() {
    let mut report = clean_mac_report();
    report.signature = CanaryHostSignature::ad_hoc(&["com.apple.security.network.client"]);

    let rejection = evaluate_curl_virtualization_canary(&report)
        .expect_err("the virtualization entitlement must be present in the signature");

    assert!(
        matches!(
            rejection,
            CurlVirtualizationCanaryRejection::EntitlementMissing(_)
        ),
        "unexpected rejection: {rejection}"
    );
    assert!(rejection
        .to_string()
        .contains(CURL_CANARY_REQUIRED_ENTITLEMENT));
}

#[test]
fn unentitled_fallback_is_rejected() {
    let mut report = clean_mac_report();
    report.execution_mode = CanaryExecutionMode::UnentitledFallback;

    let rejection = evaluate_curl_virtualization_canary(&report)
        .expect_err("an unentitled fallback must never count as a pass");

    assert!(
        matches!(
            rejection,
            CurlVirtualizationCanaryRejection::ExecutionModeRejected(_)
        ),
        "unexpected rejection: {rejection}"
    );
}

#[test]
fn non_isolated_host_process_is_rejected() {
    let mut report = clean_mac_report();
    report.execution_mode = CanaryExecutionMode::NonIsolatedHostProcess;

    let rejection = evaluate_curl_virtualization_canary(&report)
        .expect_err("running the guest workload on the host is not containment");

    assert!(
        matches!(
            rejection,
            CurlVirtualizationCanaryRejection::ExecutionModeRejected(_)
        ),
        "unexpected rejection: {rejection}"
    );
}

#[test]
fn efi_boot_loader_is_rejected() {
    let mut report = clean_mac_report();
    report.boot_loader = CanaryBootLoader::VzEfiBootLoader;

    let rejection = evaluate_curl_virtualization_canary(&report)
        .expect_err("CURL.1 froze direct kernel boot and rejected the EFI path");

    assert!(
        matches!(
            rejection,
            CurlVirtualizationCanaryRejection::BootLoaderRejected(_)
        ),
        "unexpected rejection: {rejection}"
    );
}

#[test]
fn boot_inputs_must_pin_exactly_the_three_frozen_roles() {
    let mut missing = clean_mac_report();
    missing.boot_inputs.pop();
    missing.reboot_boot_inputs.pop();
    let rejection =
        evaluate_curl_virtualization_canary(&missing).expect_err("the root disk pin is missing");
    assert!(
        matches!(
            rejection,
            CurlVirtualizationCanaryRejection::BootInputsRejected(_)
        ),
        "unexpected rejection: {rejection}"
    );
    assert!(rejection.to_string().contains("guest-root-disk"));

    let mut reordered = clean_mac_report();
    reordered.boot_inputs.swap(0, 1);
    reordered.reboot_boot_inputs.swap(0, 1);
    let rejection = evaluate_curl_virtualization_canary(&reordered)
        .expect_err("the frozen role order is part of the contract");
    assert!(
        matches!(
            rejection,
            CurlVirtualizationCanaryRejection::BootInputsRejected(_)
        ),
        "unexpected rejection: {rejection}"
    );

    let mut duplicated = clean_mac_report();
    let kernel = duplicated.boot_inputs[0].clone();
    duplicated.boot_inputs.push(kernel.clone());
    duplicated.reboot_boot_inputs.push(kernel);
    let rejection = evaluate_curl_virtualization_canary(&duplicated)
        .expect_err("a duplicated boot input is not an exact pin set");
    assert!(
        matches!(
            rejection,
            CurlVirtualizationCanaryRejection::BootInputsRejected(_)
        ),
        "unexpected rejection: {rejection}"
    );
}

#[test]
fn boot_input_digest_must_be_lowercase_hex_sha256() {
    for bad_digest in [digest('a').to_uppercase(), "abc".to_string(), digest('z')] {
        let mut report = clean_mac_report();
        report.boot_inputs[0] = CanaryBootInputPin::new(
            CanaryBootInputRole::GuestKernel,
            "guest-kernel",
            41_943_040,
            &bad_digest,
        );
        report.reboot_boot_inputs[0] = report.boot_inputs[0].clone();

        let rejection = evaluate_curl_virtualization_canary(&report)
            .expect_err("a digest that is not lowercase hex SHA-256 must be rejected");

        assert!(
            matches!(
                rejection,
                CurlVirtualizationCanaryRejection::BootInputsRejected(_)
            ),
            "unexpected rejection for {bad_digest}: {rejection}"
        );
    }
}

#[test]
fn an_empty_boot_input_is_rejected() {
    let mut report = clean_mac_report();
    report.boot_inputs[1] = CanaryBootInputPin::new(
        CanaryBootInputRole::GuestInitrd,
        "guest-initrd",
        0,
        &digest('b'),
    );
    report.reboot_boot_inputs[1] = report.boot_inputs[1].clone();

    let rejection = evaluate_curl_virtualization_canary(&report)
        .expect_err("a zero-byte boot input is not a bootable pin");

    assert!(
        matches!(
            rejection,
            CurlVirtualizationCanaryRejection::BootInputsRejected(_)
        ),
        "unexpected rejection: {rejection}"
    );
}

#[test]
fn reboot_must_reuse_the_identical_pinned_boot_inputs() {
    let mut report = clean_mac_report();
    report.reboot_boot_inputs[2] = CanaryBootInputPin::new(
        CanaryBootInputRole::GuestRootDisk,
        "guest-root-disk",
        536_870_912,
        &digest('d'),
    );

    let rejection = evaluate_curl_virtualization_canary(&report)
        .expect_err("the reboot must reuse the identical fixed boot inputs");

    assert!(
        matches!(
            rejection,
            CurlVirtualizationCanaryRejection::BootInputsRejected(_)
        ),
        "unexpected rejection: {rejection}"
    );
    assert!(rejection.to_string().contains("reboot"));
}

#[test]
fn lifecycle_must_cover_boot_shutdown_and_reboot_in_order() {
    let mut truncated = clean_mac_report();
    truncated.lifecycle.pop();
    let rejection = evaluate_curl_virtualization_canary(&truncated)
        .expect_err("the reboot boundary is part of the minimal canary");
    assert!(
        matches!(
            rejection,
            CurlVirtualizationCanaryRejection::LifecycleIncomplete(_)
        ),
        "unexpected rejection: {rejection}"
    );
    assert!(rejection.to_string().contains("reboot"));

    let mut reordered = clean_mac_report();
    reordered.lifecycle.swap(0, 1);
    let rejection = evaluate_curl_virtualization_canary(&reordered)
        .expect_err("shutdown before boot is not the frozen sequence");
    assert!(
        matches!(
            rejection,
            CurlVirtualizationCanaryRejection::LifecycleIncomplete(_)
        ),
        "unexpected rejection: {rejection}"
    );

    let mut extra = clean_mac_report();
    extra.lifecycle.push(CanaryPhaseRecord::completed(
        CanaryLifecyclePhase::Boot,
        CanaryResidueScan::empty(),
    ));
    let rejection = evaluate_curl_virtualization_canary(&extra)
        .expect_err("the minimal canary runs exactly three boundaries");
    assert!(
        matches!(
            rejection,
            CurlVirtualizationCanaryRejection::LifecycleIncomplete(_)
        ),
        "unexpected rejection: {rejection}"
    );
}

#[test]
fn a_failed_lifecycle_phase_is_rejected() {
    let mut report = clean_mac_report();
    report.lifecycle[2] = CanaryPhaseRecord::failed(
        CanaryLifecyclePhase::Reboot,
        "guest did not come back",
        CanaryResidueScan::empty(),
    );

    let rejection =
        evaluate_curl_virtualization_canary(&report).expect_err("a failed boundary is not a pass");

    assert!(
        matches!(
            rejection,
            CurlVirtualizationCanaryRejection::LifecycleIncomplete(_)
        ),
        "unexpected rejection: {rejection}"
    );
    assert!(rejection.to_string().contains("guest did not come back"));
}

#[test]
fn residue_after_any_boundary_is_rejected() {
    let mut leftover_file = clean_mac_report();
    leftover_file.lifecycle[1] = CanaryPhaseRecord::completed(
        CanaryLifecyclePhase::Shutdown,
        CanaryResidueScan::with_paths(&["/private/var/folders/boole-canary-disk.img"]),
    );
    let rejection = evaluate_curl_virtualization_canary(&leftover_file)
        .expect_err("a leftover file after shutdown is residue");
    assert!(
        matches!(
            rejection,
            CurlVirtualizationCanaryRejection::ResiduePresent(_)
        ),
        "unexpected rejection: {rejection}"
    );
    assert!(rejection.to_string().contains("boole-canary-disk.img"));

    let mut leftover_process = clean_mac_report();
    leftover_process.lifecycle[2] = CanaryPhaseRecord::completed(
        CanaryLifecyclePhase::Reboot,
        CanaryResidueScan::with_processes(&["boole-host-controller"]),
    );
    let rejection = evaluate_curl_virtualization_canary(&leftover_process)
        .expect_err("a surviving controller process after reboot is residue");
    assert!(
        matches!(
            rejection,
            CurlVirtualizationCanaryRejection::ResiduePresent(_)
        ),
        "unexpected rejection: {rejection}"
    );
    assert!(rejection.to_string().contains("boole-host-controller"));
}

#[test]
fn environment_is_checked_before_signing_and_lifecycle_evidence() {
    let mut report = clean_mac_report();
    report.clean_machine_evidence = CleanMachineEvidence::developer_machine();
    report.signature = CanaryHostSignature::unsigned();
    report.lifecycle[0] = CanaryPhaseRecord::completed(
        CanaryLifecyclePhase::Boot,
        CanaryResidueScan::with_paths(&["/tmp/boole-canary"]),
    );

    let rejection = evaluate_curl_virtualization_canary(&report)
        .expect_err("a developer machine is rejected outright");

    assert!(
        matches!(
            rejection,
            CurlVirtualizationCanaryRejection::MachineNotClean(_)
        ),
        "the machine check must precede signing and lifecycle checks: {rejection}"
    );
}

#[test]
fn an_old_macos_is_reported_before_the_machine_is_classified() {
    let mut report = clean_mac_report();
    report.macos_version = MacOsVersion::new(12, 0);
    report.clean_machine_evidence = CleanMachineEvidence::developer_machine();

    let rejection = evaluate_curl_virtualization_canary(&report)
        .expect_err("an unsupported macOS aborts the canary first");

    assert!(
        matches!(
            rejection,
            CurlVirtualizationCanaryRejection::UnsupportedMacOs(_)
        ),
        "unexpected rejection: {rejection}"
    );
}
