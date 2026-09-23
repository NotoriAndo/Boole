//! Explicit owner-wallet UX for the isolated native testnet. Work-session
//! signers and their canTransfer=false policy are deliberately not consulted.

use std::io::Read;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Context;
use boole_core::native_chain::{NativeBlock, NativeBlockTemplate, NativeChain, NativeTransfer};
use boole_core::native_network::{native_testnet, NATIVE_COIN_UNIT};
use boole_core::Hex32;
use boole_miner::ProofSigner;
use clap::{Args, Subcommand};
use serde_json::{json, Value};

#[derive(Debug, Args)]
pub(super) struct NativeArgs {
    /// Numeric loopback URL only. No DNS, proxies, redirects or public RPC.
    #[arg(long, global = true, default_value = "http://127.0.0.1:8383")]
    node: String,
    #[command(subcommand)]
    command: NativeCommand,
}

#[derive(Debug, Subcommand)]
enum NativeCommand {
    Info,
    /// Observe configured secure peers, bounded resource counters and sync states.
    Peers,
    Account {
        #[arg(long)]
        pk: String,
    },
    Transaction {
        #[arg(long)]
        txid: String,
    },
    /// Spend from an owner vault. Save the exact signed transaction BEFORE sending.
    Transfer {
        #[arg(long)]
        vault: PathBuf,
        /// Read one passphrase line from stdin, ignoring any ambient password variable.
        #[arg(long)]
        passphrase_stdin: bool,
        #[arg(long)]
        to: String,
        /// tBOOLE decimal amount (at most 8 decimal places; no floating point).
        #[arg(long)]
        amount: String,
        #[arg(long, default_value = "0.00001")]
        fee: String,
        /// Inclusive expiry height; default current height + 100.
        #[arg(long)]
        valid_before: Option<u64>,
        /// New file in an existing directory. Never overwritten; keep for recovery.
        #[arg(long)]
        outbox: PathBuf,
    },
    /// Resubmit the identical saved signature. Never automatically changes nonce.
    Submit {
        #[arg(long)]
        file: PathBuf,
    },
    /// Offline signature/format check and transaction ID. No balance or chain check.
    InspectTransfer {
        #[arg(long)]
        file: PathBuf,
    },
    /// One bounded hash attempt. Rewards are test-only and initially locked.
    Mine {
        #[arg(long)]
        vault: PathBuf,
        /// Read one passphrase line from stdin, ignoring any ambient password variable.
        #[arg(long)]
        passphrase_stdin: bool,
        #[arg(long)]
        reward_to: Option<String>,
        #[arg(long, default_value_t = 1_000_000)]
        attempts: u64,
        #[arg(long, default_value_t = 0)]
        start_nonce: u64,
        #[arg(long)]
        timestamp_ms: Option<u64>,
    },
    /// Bounded closed-local full-chain pull; validates every block before import.
    Sync {
        #[arg(long)]
        from: String,
    },
}

struct Client {
    base: String,
    http: reqwest::blocking::Client,
}
impl Client {
    fn new(base: &str) -> anyhow::Result<Self> {
        let address = base
            .strip_prefix("http://")
            .ok_or_else(|| anyhow::anyhow!("native RPC requires http://numeric-loopback:port"))?
            .parse::<SocketAddr>()
            .context("native RPC requires a numeric socket address, no path or hostname")?;
        anyhow::ensure!(
            address.ip().is_loopback() && address.port() != 0,
            "native RPC is closed-local only"
        );
        Ok(Self {
            base: format!("http://{address}"),
            http: reqwest::blocking::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(2))
                .timeout(Duration::from_secs(20))
                .build()?,
        })
    }

    fn request(&self, path: &str, body: Option<&Value>) -> anyhow::Result<Value> {
        let url = format!("{}{path}", self.base);
        let request = match body {
            Some(body) => self.http.post(url).json(body),
            None => self.http.get(url),
        };
        let response = request.send()?;
        let status = response.status();
        let limit = native_testnet().max_block_bytes() as u64 + 65_536;
        let mut bytes = Vec::new();
        response.take(limit + 1).read_to_end(&mut bytes)?;
        anyhow::ensure!(
            bytes.len() as u64 <= limit,
            "native RPC response byte limit"
        );
        anyhow::ensure!(
            status.is_success(),
            "native RPC {status}: {}",
            String::from_utf8_lossy(&bytes)
        );
        Ok(serde_json::from_slice(&bytes)?)
    }

    fn info(&self) -> anyhow::Result<Value> {
        let info = self.request("/native/info", None)?;
        let network = native_testnet();
        anyhow::ensure!(
            info["policy"] == serde_json::to_value(&network)?
                && info["genesisHash"] == network.genesis_hash().to_hex()
                && info["ready"] == true,
            "node is not the compiled native testnet; refusing to sign or submit"
        );
        Ok(info)
    }
}

fn decimal_atoms(input: &str) -> anyhow::Result<u128> {
    let (whole, fractional) = input.split_once('.').unwrap_or((input, ""));
    anyhow::ensure!(
        !whole.is_empty()
            && whole.bytes().all(|b| b.is_ascii_digit())
            && fractional.len() <= 8
            && fractional.bytes().all(|b| b.is_ascii_digit()),
        "amount requires decimal tBOOLE with at most 8 places"
    );
    let whole = whole
        .parse::<u128>()?
        .checked_mul(NATIVE_COIN_UNIT)
        .ok_or_else(|| anyhow::anyhow!("amount overflow"))?;
    let fraction = if fractional.is_empty() {
        0
    } else {
        fractional.parse::<u128>()? * 10u128.pow(8 - fractional.len() as u32)
    };
    let amount = whole
        .checked_add(fraction)
        .ok_or_else(|| anyhow::anyhow!("amount overflow"))?;
    anyhow::ensure!(
        amount <= native_testnet().total_supply(),
        "amount exceeds testnet supply cap"
    );
    Ok(amount)
}

fn number(value: &Value, field: &str) -> anyhow::Result<u64> {
    let text = value[field]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing canonical {field}"))?;
    let number: u64 = text.parse()?;
    anyhow::ensure!(number.to_string() == text, "noncanonical {field}");
    Ok(number)
}

fn owner_signer(vault: &Path, passphrase_stdin: bool) -> anyhow::Result<boole_miner::AgentSigner> {
    let agent = super::resolve_wallet_agent_binary()?
        .to_string_lossy()
        .into_owned();
    if passphrase_stdin {
        return boole_miner::AgentSigner::from_stdin(agent, vault.to_path_buf())
            .map_err(anyhow::Error::msg);
    }
    let passphrase = std::env::var("BOOLE_WALLET_PASSPHRASE")
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "owner vault requires --passphrase-stdin or BOOLE_WALLET_PASSPHRASE (never argv)"
            )
        })?;
    Ok(boole_miner::AgentSigner::new(
        agent,
        vault.to_path_buf(),
        passphrase,
    ))
}

fn new_outbox(path: &Path) -> anyhow::Result<PathBuf> {
    let filename = path
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("outbox requires filename"))?;
    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .canonicalize()?;
    let path = parent.join(filename);
    match std::fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(path),
        Err(error) => Err(error.into()),
        Ok(_) => anyhow::bail!("outbox already exists; query or submit that exact file instead"),
    }
}

fn read_transfer(path: &Path) -> anyhow::Result<NativeTransfer> {
    let metadata = std::fs::symlink_metadata(path)?;
    anyhow::ensure!(
        metadata.is_file()
            && !metadata.file_type().is_symlink()
            && metadata.len() <= native_testnet().max_transfer_bytes() as u64,
        "invalid or oversized signed transfer file"
    );
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(native_testnet().max_transfer_bytes() as u64 + 1)
        .read_to_end(&mut bytes)?;
    anyhow::ensure!(
        bytes.len() <= native_testnet().max_transfer_bytes(),
        "signed transfer byte limit"
    );
    let transfer: NativeTransfer = serde_json::from_slice(&bytes)?;
    transfer.validated_fields()?;
    Ok(transfer)
}

fn inspect_transfer(file: &Path) -> anyhow::Result<Value> {
    let transfer = read_transfer(file)?;
    let network = native_testnet();
    Ok(json!({
        "schema": "boole.native.transfer.inspection.v1",
        "verification": "signature_and_format_only",
        "chainStatus": "not_checked",
        "networkId": transfer.network_id,
        // This identifies our compiled policy, not an independently signed
        // genesis field in the transfer envelope or a contacted node's head.
        "compiledGenesisHash": network.genesis_hash().to_hex(),
        "txid": transfer.id().to_hex(),
        "from": transfer.payload.from, "to": transfer.payload.to,
        "amountAtoms": transfer.payload.amount, "feeAtoms": transfer.payload.fee,
        "nonce": transfer.payload.nonce, "validBefore": transfer.payload.valid_before
    }))
}

pub(super) fn run(args: NativeArgs) -> anyhow::Result<()> {
    if let NativeCommand::InspectTransfer { file } = &args.command {
        println!("{}", serde_json::to_string(&inspect_transfer(file)?)?);
        return Ok(());
    }
    let client = Client::new(&args.node)?;
    let info = client.info()?;
    let result = match args.command {
        NativeCommand::Info => info,
        NativeCommand::Peers => client.request("/native/peers", None)?,
        NativeCommand::Account { pk } => {
            anyhow::ensure!(
                Hex32::from_hex(&pk)?.to_hex() == pk,
                "noncanonical account key"
            );
            client.request(&format!("/native/accounts/{pk}"), None)?
        }
        NativeCommand::Transaction { txid } => {
            anyhow::ensure!(
                Hex32::from_hex(&txid)?.to_hex() == txid,
                "noncanonical transaction ID"
            );
            client.request(&format!("/native/transactions/{txid}"), None)?
        }
        NativeCommand::Transfer {
            vault,
            passphrase_stdin,
            to,
            amount,
            fee,
            valid_before,
            outbox,
        } => {
            let outbox = new_outbox(&outbox)?;
            anyhow::ensure!(
                Hex32::from_hex(&to)?.to_hex() == to,
                "noncanonical destination key"
            );
            let amount = decimal_atoms(&amount)?;
            let fee = decimal_atoms(&fee)?;
            anyhow::ensure!(
                amount > 0 && fee >= native_testnet().minimum_fee(),
                "zero amount or fee below testnet minimum"
            );
            let signer = owner_signer(&vault, passphrase_stdin)?;
            let from = signer.pk_hex().map_err(anyhow::Error::msg)?;
            let account = client.request(&format!("/native/accounts/{from}"), None)?;
            let nonce = number(&account, "pendingNonce")?;
            let height = number(&account, "height")?;
            let valid_before = valid_before.unwrap_or(
                height
                    .checked_add(100)
                    .ok_or_else(|| anyhow::anyhow!("expiry height overflow"))?,
            );
            anyhow::ensure!(
                valid_before > height,
                "transfer already expired for the next block"
            );
            let payload = json!({"schema": "boole.transfer.v1", "from": from, "to": to,
                "amount": amount.to_string(), "fee": fee.to_string(), "nonce": nonce.to_string(), "validBefore": valid_before.to_string()});
            let signed = signer
                .sign_payload(&payload, native_testnet().network_id())
                .map_err(anyhow::Error::msg)?;
            let transfer = NativeTransfer::try_from(&signed)?;
            transfer.validated_fields()?;
            super::atomic_create_0600(&outbox, &serde_json::to_vec(&transfer)?)
                .context("signed outbox durability failed; nothing was broadcast")?;
            let id = transfer.id().to_hex();
            eprintln!("Saved signed test-coin transfer {id} to {}. Keep this file; confirmation is not finality.", outbox.display());
            client
                .request("/native/transfers", Some(&serde_json::to_value(transfer)?))
                .with_context(|| {
                    format!(
                        "submission uncertain or rejected; query {id} and resubmit only {}",
                        outbox.display()
                    )
                })?
        }
        NativeCommand::Submit { file } => {
            let transfer = read_transfer(&file)?;
            client.request("/native/transfers", Some(&serde_json::to_value(transfer)?))?
        }
        NativeCommand::InspectTransfer { .. } => unreachable!("handled before RPC creation"),
        NativeCommand::Mine {
            vault,
            passphrase_stdin,
            reward_to,
            attempts,
            start_nonce,
            timestamp_ms,
        } => {
            anyhow::ensure!(
                (1..=10_000_000).contains(&attempts),
                "attempts must be 1..=10000000"
            );
            let signer = owner_signer(&vault, passphrase_stdin)?;
            let producer = signer.pk_hex().map_err(anyhow::Error::msg)?;
            let reward = reward_to.unwrap_or_else(|| producer.clone());
            anyhow::ensure!(
                Hex32::from_hex(&reward)?.to_hex() == reward,
                "noncanonical reward key"
            );
            let now = u64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis())?;
            let timestamp = timestamp_ms.unwrap_or(now.max(number(&info, "minimumTimestampMs")?));
            let template: NativeBlockTemplate = serde_json::from_value(client.request(
                "/native/template",
                Some(
                    &json!({"producerPk": producer, "rewardPk": reward, "timestampMs": timestamp}),
                ),
            )?)?;
            anyhow::ensure!(template.header.network_id == native_testnet().network_id()
                && template.header.genesis_hash == native_testnet().genesis_hash().to_hex()
                && template.header.producer_pk == producer && template.header.reward_pk == reward
                && template.header.timestamp_ms == timestamp
                && template.header.previous_hash == info["headHash"].as_str().unwrap_or("")
                && template.header.height == number(&info, "height")?.checked_add(1).ok_or_else(|| anyhow::anyhow!("height overflow"))?,
                "node template changed the requested mining authority or head; retry with a fresh template");
            for transfer in &template.transfers {
                transfer.validated_fields()?;
            }
            let block = template.mine(start_nonce, attempts)?.ok_or_else(|| {
                anyhow::anyhow!("bounded work exhausted; no block was signed or submitted")
            })?;
            let auth = signer
                .sign_payload(
                    &block.authorization_payload()?,
                    native_testnet().network_id(),
                )
                .map_err(anyhow::Error::msg)?;
            let block = block.authorize(&auth)?;
            client.request("/native/blocks", Some(&serde_json::to_value(block)?))?
        }
        NativeCommand::Sync { from } => sync(&client, &Client::new(&from)?)?,
    };
    println!("{}", serde_json::to_string(&result)?);
    Ok(())
}

fn sync(target: &Client, source: &Client) -> anyhow::Result<Value> {
    use boole_node::{MAX_NATIVE_SYNC_BLOCKS, MAX_NATIVE_SYNC_BYTES};
    let info = source.info()?;
    let height = number(&info, "height")?;
    anyhow::ensure!(
        height <= MAX_NATIVE_SYNC_BLOCKS as u64,
        "local full-chain sync block limit; no partial import"
    );
    let mut blocks: Vec<NativeBlock> = Vec::new();
    let mut bytes = 2usize;
    for height in 1..=height {
        let block = source.request(&format!("/native/blocks/{height}"), None)?;
        bytes += serde_json::to_vec(&block)?.len() + 1;
        anyhow::ensure!(
            bytes <= MAX_NATIVE_SYNC_BYTES,
            "local full-chain sync byte limit; no partial import"
        );
        blocks.push(serde_json::from_value(block)?);
    }
    let chain = NativeChain::replay(&blocks)?;
    anyhow::ensure!(
        info["headHash"] == chain.head_hash().to_hex()
            && source.info()?["headHash"] == info["headHash"],
        "source changed during sync; no import"
    );
    target.request("/native/chain", Some(&serde_json::to_value(blocks)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn coin_input_never_uses_float_rounding_or_exponents() {
        assert_eq!(decimal_atoms("0.00000001").unwrap(), 1);
        assert_eq!(decimal_atoms("1.25").unwrap(), 125_000_000);
        assert_eq!(
            decimal_atoms("1000000000").unwrap(),
            native_testnet().total_supply()
        );
        for invalid in [
            "1e8",
            "-1",
            "+1",
            "NaN",
            "0.000000001",
            "1000000000.00000001",
            "1.2.3",
            " 1",
            "",
        ] {
            assert!(decimal_atoms(invalid).is_err(), "{invalid}");
        }
    }
    #[test]
    fn native_client_refuses_dns_public_addresses_and_url_credentials() {
        for invalid in [
            "https://127.0.0.1:8383",
            "http://localhost:8383",
            "http://0.0.0.0:8383",
            "http://192.0.2.1:8383",
            "http://user:pass@127.0.0.1:8383",
            "http://127.0.0.1:8383/path",
        ] {
            assert!(Client::new(invalid).is_err(), "{invalid}");
        }
    }
}
