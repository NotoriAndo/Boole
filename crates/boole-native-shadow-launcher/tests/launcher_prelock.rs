#[cfg(not(target_os = "linux"))]
#[test]
fn fixed_launcher_prelock_check_fails_closed_off_linux() {
    assert!(matches!(
        boole_native_shadow_launcher::startup::verify_fixed_launcher_prelock_prerequisites(),
        Err(boole_native_shadow_launcher::startup::LauncherPrelockError::UnsupportedPlatform)
    ));
}
