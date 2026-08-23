use boole_native_shadow_launcher::qualification::{
    serve_one_fixed_unix_qualification, FixedQualificationListenerError,
    VerifiedQualificationStartup,
};

#[test]
fn public_listener_consumes_exactly_one_complete_readiness_token() {
    let _entrypoint: fn(
        VerifiedQualificationStartup,
    ) -> Result<(), FixedQualificationListenerError> = serve_one_fixed_unix_qualification;
}
