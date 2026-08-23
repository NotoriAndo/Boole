#[cfg(not(target_os = "linux"))]
#[test]
fn fixed_launcher_privilege_check_fails_closed_off_linux() {
    assert!(matches!(
        boole_native_shadow_launcher::privilege::verify_fixed_launcher_privilege(),
        Err(boole_native_shadow_launcher::privilege::LauncherPrivilegeError::UnsupportedPlatform)
    ));
}
