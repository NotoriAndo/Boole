#[cfg(target_os = "linux")]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    anyhow::ensure!(
        std::env::args_os().len() == 1,
        "canary node takes no request-selected configuration"
    );
    boole_node::serve_installed_fresh_answer_canary().await
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("fresh-answer canary requires qualified Linux; no host fallback");
    std::process::exit(2);
}
