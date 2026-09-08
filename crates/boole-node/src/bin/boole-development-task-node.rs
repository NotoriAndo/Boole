#[cfg(target_os = "linux")]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    anyhow::ensure!(
        std::env::args_os().len() == 1,
        "development task node takes no request-selected configuration"
    );
    boole_node::serve_installed_development_task().await
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("development task admission requires qualified Linux; no host fallback");
    std::process::exit(2);
}
