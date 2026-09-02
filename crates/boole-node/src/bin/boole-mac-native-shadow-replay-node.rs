#[cfg(target_os = "macos")]
use std::path::PathBuf;

#[cfg(target_os = "macos")]
use clap::Parser;

#[cfg(target_os = "macos")]
#[derive(Debug, Parser)]
#[command(
    name = "boole-mac-native-shadow-replay-node",
    about = "Serve the installed closed-local native-shadow product on macOS"
)]
struct Args {
    #[arg(long)]
    install_root: PathBuf,
    #[arg(long)]
    runtime_root: PathBuf,
    #[arg(long)]
    journal_path: PathBuf,
    #[arg(long)]
    product_trust_root_key_id: String,
    #[arg(long)]
    product_trust_root_public_key: String,
    #[arg(long)]
    guest_trust_root_key_id: String,
    #[arg(long)]
    guest_trust_root_public_key: String,
}

#[cfg(target_os = "macos")]
#[tokio::main]
#[allow(unsafe_code)]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let product_trust_root = boole_core::CurlProductReleaseTrustRoot::new(
        &args.product_trust_root_key_id,
        &args.product_trust_root_public_key,
    )?;
    let guest_trust_root = boole_core::NativeShadowUpdateTrustRoot::new(
        &args.guest_trust_root_key_id,
        &args.guest_trust_root_public_key,
    )?;
    // SAFETY: these calls only read the immutable process credentials and
    // retain no pointer or borrowed OS storage.
    let (uid, gid) = unsafe { (libc::geteuid(), libc::getegid()) };
    let config = boole_node::InstalledMacReplayConfig::new(
        args.install_root,
        args.runtime_root,
        args.journal_path,
        product_trust_root,
        guest_trust_root,
        uid,
        gid,
    )?;
    boole_node::serve_installed_mac_closed_local_native_shadow_replay(config).await
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("boole-mac-native-shadow-replay-node requires macOS");
    std::process::exit(2);
}
