use crate::support::m2::zstd_binary;
use anyhow::{ensure, Context, Result};
use boole_derivation::m2::Config;
use serde_json::{json, Value};
use std::{
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
pub struct Server {
    child: Child,
    input: Option<ChildStdin>,
    output: mpsc::Receiver<String>,
    next: u64,
    pids: PathBuf,
}
pub fn client_binary() -> PathBuf {
    std::env::var_os("BOOLE_DEVNET_MCP_BINARY")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_boole-devnet-mcp")))
}
impl Server {
    pub fn start(
        root: &Path,
        c: &Config,
        user: &str,
        state: &Path,
        extra: &[String],
    ) -> Result<Self> {
        fs::create_dir_all(root)?;
        let config = root.join("config.json");
        fs::write(&config, serde_json::to_vec(c)?)?;
        let codec = zstd_binary()?;
        let mut child = Command::new(client_binary())
            .args([
                "--devnet",
                "--l1-rpc",
                "http://127.0.0.1:28545",
                "--node-rpc",
                "http://127.0.0.1:29545",
                "--account",
                user,
                "--m2-config",
            ])
            .arg(config)
            .arg("--state-dir")
            .arg(state)
            .arg("--zstd")
            .arg(codec)
            .args(extra)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(File::create(root.join("stderr.log"))?)
            .spawn()?;
        let pids = root.join("owned-pids.jsonl");
        record_pid(&pids, child.id(), "started")?;
        let input = child.stdin.take().context("MCP stdin")?;
        let stdout = child.stdout.take().context("MCP stdout")?;
        let (sender, output) = mpsc::channel();
        let mut transcript = File::create(root.join("stdout.log"))?;
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                match line {
                    Ok(s) => {
                        if writeln!(transcript, "{s}").is_err() {
                            break;
                        }
                        if sender.send(s).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });
        let mut s = Self {
            child,
            input: Some(input),
            output,
            next: 0,
            pids,
        };
        let init=s.rpc("initialize",json!({"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"devnet-live-test","version":"1"}}))?;
        ensure!(
            init["result"]["serverInfo"]["name"] == "boole-devnet-mcp",
            "MCP initialize"
        );
        let list = s.rpc("tools/list", json!({}))?;
        ensure!(
            list["result"]["tools"].as_array().context("tools")?.len() == 6,
            "six tools"
        );
        Ok(s)
    }
    pub fn rpc(&mut self, method: &str, params: Value) -> Result<Value> {
        self.next += 1;
        let request = json!({"jsonrpc":"2.0","id":self.next,"method":method,"params":params});
        writeln!(self.input.as_mut().context("MCP stopped")?, "{request}")?;
        self.input.as_mut().context("MCP input")?.flush()?;
        let response: Value = serde_json::from_str(
            &self
                .output
                .recv_timeout(Duration::from_secs(15))
                .context("MCP response timeout/exit")?,
        )?;
        ensure!(response["id"] == self.next, "MCP response id");
        Ok(response)
    }
    pub fn tool(&mut self, name: &str, args: Value) -> Result<Value> {
        let out = self.rpc("tools/call", json!({"name":name,"arguments":args}))?;
        ensure!(
            out["result"]["isError"] == false,
            "MCP tool rejected: {out}"
        );
        Ok(serde_json::from_str(
            out["result"]["content"][0]["text"]
                .as_str()
                .context("tool text")?,
        )?)
    }
    pub fn submission(&mut self, id: &str) -> Result<Value> {
        self.tool("boole.devnet_submission", json!({"submission_id":id}))
    }
    pub fn posted(&mut self, id: &str, stage: &str) -> Result<String> {
        let start = Instant::now();
        loop {
            let view = self.submission(id)?;
            if let Some(tx) = view["transactions"]
                .as_array()
                .context("transactions")?
                .iter()
                .find(|x| x["stage"] == stage)
                .and_then(|x| x["transaction_hash"].as_str())
            {
                return Ok(tx.into());
            }
            ensure!(
                start.elapsed() < Duration::from_secs(5),
                "MCP did not post {stage}: {view}"
            );
            thread::sleep(Duration::from_millis(20));
        }
    }
    pub fn stop(&mut self) -> Result<()> {
        self.input.take();
        let start = Instant::now();
        loop {
            if let Some(status) = self.child.try_wait()? {
                record_pid(&self.pids, self.child.id(), "stopped")?;
                ensure!(status.success(), "MCP exit {status}");
                return Ok(());
            }
            ensure!(
                start.elapsed() < Duration::from_secs(3),
                "MCP EOF stop timeout"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = record_pid(&self.pids, self.child.id(), "stopped");
    }
}
fn record_pid(path: &Path, pid: u32, state: &str) -> Result<()> {
    let line = json!({"name":"mcp1-stdio","pid":pid,"state":state});
    writeln!(
        OpenOptions::new().create(true).append(true).open(path)?,
        "{line}"
    )?;
    if let Some(p) = std::env::var_os("BOOLE_DEVNET_OWNED_PIDS") {
        writeln!(
            OpenOptions::new().create(true).append(true).open(p)?,
            "{line}"
        )?;
    }
    Ok(())
}
