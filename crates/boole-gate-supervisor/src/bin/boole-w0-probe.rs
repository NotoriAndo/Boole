//! Qualification fixture executable, never selected by the real-gate dispatcher.
#[cfg(target_os = "linux")]
#[path = "../w0_fixture.rs"]
mod fixture;
fn main() {
    #[cfg(target_os = "linux")]
    if let Err(error) = fixture::main() {
        eprintln!("W0 fixture unavailable: {error:#}");
        std::process::exit(2);
    }
    #[cfg(not(target_os = "linux"))]
    {
        eprintln!("W0 fixture requires Linux; no host fallback");
        std::process::exit(2);
    }
}
