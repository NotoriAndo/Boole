#[tokio::main]
async fn main() -> anyhow::Result<()> {
    boole_node::serve_installed_closed_local_native_shadow_replay().await
}
