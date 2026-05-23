use clap::{Parser, Subcommand};
use std::fs::OpenOptions;
use std::io::{Read as _, Write as _};
use std::net::TcpStream;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

#[derive(Debug, Parser)]
#[command(name = "boole")]
#[command(about = "Boole native CLI migration spike")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print CLI version information.
    Version {
        /// Emit JSON output.
        #[arg(long)]
        json: bool,
    },
    /// Chain inspection commands.
    Chain {
        #[command(subcommand)]
        command: ChainCommand,
    },
    /// Local node lifecycle commands.
    Node {
        #[command(subcommand)]
        command: NodeCommand,
    },
    /// Block inspection commands hitting a running boole-node.
    Block {
        #[command(subcommand)]
        command: BlockCommand,
    },
    /// Account balance lookup against a running boole-node.
    Account {
        #[command(subcommand)]
        command: AccountCommand,
    },
    /// Read-only reputation ledger inspection.
    Reputation {
        #[command(subcommand)]
        command: ReputationCommand,
    },
    /// Work manifest catalog queries against a running boole-node.
    Work {
        #[command(subcommand)]
        command: WorkCommand,
    },
    /// Bounty catalog queries against a running boole-node.
    Bounty {
        #[command(subcommand)]
        command: BountyCommand,
    },
    /// Local key management. Storage at `$BOOLE_KEYS_DIR` (env override) or
    /// `$HOME/.boole/keys` (default), mode 0600 per file.
    Keys {
        #[command(subcommand)]
        command: KeysCommand,
    },
    /// Local agent session-key policy management. Storage at
    /// `$BOOLE_SESSIONS_DIR` (env override) or `$HOME/.boole/sessions`
    /// (default), mode 0600 per file. The on-disk envelope carries the
    /// ed25519 session secret seed; stdout never echoes it (W0 redaction
    /// invariant).
    SessionKey {
        #[command(subcommand)]
        command: SessionKeyCommand,
    },
    /// Local policy-bound signer. Authorizes a request against a stored
    /// session policy, checks nonce reuse, then ed25519-signs the payload
    /// with the session secret seed loaded from `$BOOLE_SESSIONS_DIR`.
    /// The session secret seed never leaves disk and is never echoed.
    Signer {
        #[command(subcommand)]
        command: SignerCommand,
    },
    /// Boole-v3.1.1 miner: state init/inspection, mining loop, and bounty
    /// submission. Delegates to the `boole-miner` library so the standalone
    /// `boole-miner` binary and `boole mine ...` share the same code paths.
    Mine {
        #[command(subcommand)]
        command: boole_miner::cli::MineCommand,
    },
    /// Offline durable-state inspection. Replays the durable block log via
    /// the same recovery path the node uses at boot, without acquiring the
    /// state-dir lock. Safe to run while a node is up.
    State {
        #[command(subcommand)]
        command: StateCommand,
    },
}

#[derive(Debug, Subcommand)]
enum ChainCommand {
    /// Replay a protocol fixture or block log and print final state.
    Replay {
        /// Path to replay fixture JSON.
        #[arg(long)]
        fixture: PathBuf,
        /// Emit JSON output.
        #[arg(long)]
        json: bool,
    },
    /// Audit submit receipt ledger against a persisted block NDJSON log.
    AuditReceipts {
        /// Path to persisted blocks NDJSON.
        #[arg(long)]
        blocks: PathBuf,
        /// Path to submit receipt ledger NDJSON.
        #[arg(long)]
        receipts: PathBuf,
        /// Emit JSON output.
        #[arg(long)]
        json: bool,
    },
    /// Summarize audited receipt settlement/reputation deltas without mutating ledgers.
    SettlementReport {
        /// Path to persisted blocks NDJSON.
        #[arg(long)]
        blocks: PathBuf,
        /// Path to submit receipt ledger NDJSON.
        #[arg(long)]
        receipts: PathBuf,
        /// Export read-only reputation event NDJSON rows derived from settlement.reputationDeltas.
        #[arg(long)]
        export_reputation_events: Option<PathBuf>,
        /// Emit JSON output.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum NodeCommand {
    /// Spawn a local boole-node process bound to --port writing into --data-dir.
    Start {
        /// TCP port to bind (env: PORT).
        #[arg(long)]
        port: Option<u16>,
        /// Directory holding node state (block store NDJSON).
        #[arg(long)]
        data_dir: PathBuf,
        /// Scenario fixture path; defaults to fixtures/protocol/runtime-smoke/v1.json.
        #[arg(long)]
        scenario: Option<PathBuf>,
        /// Override the scenario's genesis_c (env: GENESIS_C).
        #[arg(long)]
        genesis: Option<String>,
        /// Cap requests served before exiting (smoke/test convenience).
        #[arg(long)]
        max_requests: Option<usize>,
    },
}

#[derive(Debug, Subcommand)]
enum KeysCommand {
    /// Generate a new local key and persist it under the keys directory.
    New {
        /// Human label for the key (matches `[a-zA-Z0-9_-]+`).
        #[arg(long)]
        id: String,
        /// Use a deterministic seed derived from `--id` instead of OS random.
        /// Intended for fixtures and reproducible tests.
        #[arg(long)]
        dev: bool,
        /// Print the envelope to stdout but skip the disk write.
        #[arg(long)]
        dry_run: bool,
    },
    /// Enumerate keys under the keys directory, sorted by id.
    List,
    /// Print a single key envelope by id.
    Show {
        /// Id of the key to read.
        #[arg(long)]
        id: String,
    },
    /// Sign a JSON payload with a stored v2 key. Default stdout is the bare
    /// hex64 ed25519 signature; `--json` emits the full `boole.signed.v1`
    /// envelope.
    Sign {
        /// Id of the key to sign with (must be a v2 envelope).
        #[arg(long)]
        id: String,
        /// JSON payload to sign — accepts an inline JSON string or a path to
        /// a JSON file.
        #[arg(long)]
        payload: String,
        /// Emit the full `boole.signed.v1` envelope instead of just the
        /// signature.
        #[arg(long)]
        json: bool,
    },
    /// Verify a hex64 ed25519 signature against a hex32 public key and a
    /// JSON payload. Stateless: never touches the keys directory.
    Verify {
        /// 32-byte ed25519 public key (64 lowercase hex chars).
        #[arg(long)]
        pk: String,
        /// 64-byte ed25519 signature (128 lowercase hex chars).
        #[arg(long)]
        signature: String,
        /// JSON payload to verify against — inline JSON or a file path.
        #[arg(long)]
        payload: String,
        /// Emit the full result as a typed envelope instead of the bare
        /// `valid`/`invalid` word.
        #[arg(long)]
        json: bool,
    },
    /// **UNSAFE** — print the full stored envelope including the ed25519
    /// secret seed `sk`. The only path that re-exposes the secret after
    /// W0.2's redaction. Use for explicit backup / dev workflows only.
    /// Output carries `"unsafe": true` and a warning string.
    ExportSecret {
        /// Id of the key whose secret seed to export.
        #[arg(long)]
        id: String,
    },
}

#[derive(Debug, Subcommand)]
enum SessionKeyCommand {
    /// Create a new local agent session-key policy. The session signing key
    /// is freshly generated; the secret seed is persisted to disk under
    /// `$BOOLE_SESSIONS_DIR` and never printed to stdout.
    Create {
        /// Local-only session (no node registration). Required in this
        /// slice — the node-backed path lands in N1.x.
        #[arg(long)]
        local: bool,
        /// Stable id for the session policy file (filename and lookup key).
        #[arg(long)]
        id: String,
        /// Owner key id (resolved against `$BOOLE_KEYS_DIR`). The owner pk
        /// is also used as the fixed reward recipient in this slice.
        #[arg(long = "owner-id")]
        owner_id: String,
        /// Agent key id (resolved against `$BOOLE_KEYS_DIR`). The agent pk
        /// is the on-chain identity the session works on behalf of.
        #[arg(long = "agent-id")]
        agent_id: String,
        /// Route the session may sign requests for. Repeat for multiple routes.
        #[arg(long = "allowed-route")]
        allowed_routes: Vec<String>,
        /// Family id the session may submit work for.
        #[arg(long = "allowed-family")]
        allowed_family: String,
        /// Verifier id the session may pay verification fees to.
        #[arg(long = "allowed-verifier")]
        allowed_verifier: String,
        /// Maximum fee per request (decimal u128 string).
        #[arg(long = "max-fee")]
        max_fee: String,
        /// Daily fee cap (decimal u128 string).
        #[arg(long = "daily-fee-cap")]
        daily_fee_cap: String,
        /// Expiry height (`activation_height` defaults to 0 in this slice).
        #[arg(long = "expiry-height")]
        expiry_height: u64,
    },
    /// Print the public policy view for an existing local session-key file.
    /// Mirrors `session-key create` stdout — the secret seed (`sessionSk`)
    /// is never emitted.
    Inspect {
        /// Session id (filename stem under `$BOOLE_SESSIONS_DIR`).
        #[arg(long)]
        id: String,
    },
    /// Mark a local session-key file as revoked. Rewrites the envelope in
    /// place via `atomic_write_0600` with `revoked: true`. This is local
    /// only — the authoritative on-chain revocation lands with N1.x.
    Revoke {
        /// Local-only revocation. Required in this slice — the node-backed
        /// path lands in N1.x.
        #[arg(long)]
        local: bool,
        /// Session id (filename stem under `$BOOLE_SESSIONS_DIR`).
        #[arg(long)]
        id: String,
    },
}

#[derive(Debug, Subcommand)]
enum SignerCommand {
    /// Sign a work payload with a local session key after authorizing the
    /// request against the session's policy. Refuses on policy violation,
    /// duplicate nonce, missing session, or malformed inputs.
    SignWork {
        /// Session id (filename stem under `$BOOLE_SESSIONS_DIR`).
        #[arg(long = "session-id")]
        session_id: String,
        /// Route the request will hit (must match policy `allowed_routes`).
        #[arg(long)]
        route: String,
        /// Family id (must match policy `allowed_family_ids`).
        #[arg(long)]
        family: String,
        /// Verifier id (must match policy `allowed_verifier_ids`).
        #[arg(long)]
        verifier: String,
        /// Fee for this request (decimal u128 string; must be <= policy max).
        #[arg(long)]
        fee: String,
        /// 32-byte lowercase hex request hash (the payload pre-image
        /// commitment the caller binds to this signature).
        #[arg(long = "request-hash")]
        request_hash: String,
        /// Per-session nonce; must not have been seen by this signer before.
        #[arg(long)]
        nonce: String,
        /// JSON payload to sign (literal JSON or `@path` to load from file,
        /// matching `keys sign`).
        #[arg(long)]
        payload: String,
        /// Emit the full `boole.signed.v1` envelope as JSON. Without this
        /// flag, only the bare hex64 signature prints to stdout.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum AccountCommand {
    /// Print the reward balance for `--pk` from a node's `/account/{pk}/balance`.
    Balance {
        /// 32-byte public key hex (64 lowercase hex chars).
        #[arg(long)]
        pk: String,
        /// Base URL of the boole-node (default http://127.0.0.1:8080).
        #[arg(long)]
        node: Option<String>,
        /// Print the full server envelope as JSON instead of the bare balance.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum ReputationCommand {
    /// Inspect a file-backed reputation ledger for an agent key without mutating it.
    Inspect {
        /// Path to reputation NDJSON ledger.
        #[arg(long)]
        ledger: PathBuf,
        /// 32-byte agent public key hex (64 lowercase hex chars).
        #[arg(long = "agent-pk")]
        agent_pk: String,
        /// Emit JSON output.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum WorkCommand {
    /// List the static work manifests served by `/work`.
    List {
        /// Base URL of the boole-node (default http://127.0.0.1:8080).
        #[arg(long)]
        node: Option<String>,
        /// Print the full server envelope as JSON instead of the terse table.
        #[arg(long)]
        json: bool,
    },
    /// Fetch a single work manifest by id from `/work/{id}`.
    Get {
        /// Work id to look up (server returns 404 typed envelope on miss).
        #[arg(long)]
        id: String,
        /// Base URL of the boole-node (default http://127.0.0.1:8080).
        #[arg(long)]
        node: Option<String>,
        /// Print the full server envelope as JSON instead of the bare verifier hash.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum BountyCommand {
    /// List the static bounties served by `/bounties`.
    List {
        /// Base URL of the boole-node (default http://127.0.0.1:8080).
        #[arg(long)]
        node: Option<String>,
        /// Print the full server envelope as JSON instead of the terse table.
        #[arg(long)]
        json: bool,
    },
    /// Fetch a single bounty by id from `/bounties/{id}`.
    Get {
        /// Bounty id to look up (server returns 404 typed envelope on miss).
        #[arg(long)]
        id: String,
        /// Base URL of the boole-node (default http://127.0.0.1:8080).
        #[arg(long)]
        node: Option<String>,
        /// Print the full server envelope as JSON instead of the bare verifier hash.
        #[arg(long)]
        json: bool,
    },
    /// Submit a proof envelope to `POST /bounties/{id}/proof`. P1.6d —
    /// the route requires a `boole.signed.v1` outer envelope around a
    /// `boole.bounty.proof.v1` payload; the prover pk is derived from
    /// `--signing-key` (the envelope signer must equal the claimed
    /// prover, so a separate `--prover` flag would be redundant and a
    /// foot-gun if mismatched).
    Submit {
        /// Bounty id whose verifier will judge the envelope.
        #[arg(long)]
        id: String,
        /// 32-byte lowercase hex hash uniquely identifying the proof
        /// payload (used for dedup).
        #[arg(long = "proof-hash")]
        proof_hash: String,
        /// Id of the stored v2 key used to sign the proof envelope. The
        /// derived ed25519 public key becomes the payload `prover` and
        /// envelope `pk`.
        #[arg(long = "signing-key")]
        signing_key: String,
        /// Path to a JSON envelope file or an inline JSON string. The
        /// envelope shape is verifier-specific (e.g. `{"leanSource": "..."}`
        /// for the Lean verifier).
        #[arg(long)]
        envelope: String,
        /// Base URL of the boole-node (default http://127.0.0.1:8080).
        #[arg(long)]
        node: Option<String>,
        /// Print the full server envelope as JSON instead of the bare
        /// status word (`solved`/`open`/`duplicate`).
        #[arg(long)]
        json: bool,
    },
    /// Announce a new bounty: build a `boole.bounty.announce.v1` payload,
    /// sign it locally with a stored v2 key, and POST the
    /// `boole.signed.v1` envelope to `/bounties`.
    Announce {
        /// New bounty id (1-128 printable ASCII chars without whitespace).
        #[arg(long)]
        id: String,
        /// Bounty domain string (e.g. `code.spec-template`).
        #[arg(long)]
        domain: String,
        /// 32-byte lowercase hex hash of the problem statement.
        #[arg(long = "problem-hash")]
        problem_hash: String,
        /// Verifier kind (e.g. `lean`, `mock-accept`).
        #[arg(long = "verifier-kind")]
        verifier_kind: String,
        /// Verifier metadata as inline JSON or a path to a JSON file.
        #[arg(long = "verifier-metadata")]
        verifier_metadata: String,
        /// Reward amount as a positive base-10 integer (u128 string).
        #[arg(long)]
        reward: String,
        /// Deadline as unix milliseconds.
        #[arg(long)]
        deadline: u64,
        /// Optional override for the announce timestamp (unix ms). Defaults
        /// to the current wall-clock time. Surfaced for fixture
        /// reproducibility.
        #[arg(long)]
        ts: Option<u64>,
        /// Id of the stored v2 key used to sign the payload.
        #[arg(long = "signing-key")]
        signing_key: String,
        /// Base URL of the boole-node (default http://127.0.0.1:8080).
        #[arg(long)]
        node: Option<String>,
        /// Print the full server envelope as JSON instead of the bare
        /// bounty id.
        #[arg(long)]
        json: bool,
    },
    /// Change a bounty's lifecycle status. Builds a
    /// `boole.bounty.status.v1` payload, signs it locally with a stored
    /// v2 key, and POSTs the `boole.signed.v1` envelope to
    /// `/bounties/{id}/status`.
    Status {
        /// Bounty id whose status should change.
        #[arg(long)]
        id: String,
        /// Target status (one of `open`, `solved`, `expired`, `withdrawn`).
        #[arg(long = "new-status", value_parser = ["open", "solved", "expired", "withdrawn"])]
        new_status: String,
        /// Optional operator-supplied free-form reason recorded in the audit log.
        #[arg(long)]
        reason: Option<String>,
        /// Optional override for the status timestamp (unix ms). Defaults
        /// to current wall-clock time. Surfaced for fixture reproducibility.
        #[arg(long)]
        ts: Option<u64>,
        /// Id of the stored v2 key used to sign the payload.
        #[arg(long = "signing-key")]
        signing_key: String,
        /// Base URL of the boole-node (default http://127.0.0.1:8080).
        #[arg(long)]
        node: Option<String>,
        /// Print the full server envelope as JSON instead of the bare
        /// new status word.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum StateCommand {
    /// Replay a durable block NDJSON log and report height + latest c.
    /// Same recovery shape `boole-node` uses at boot, but read-only:
    /// no lock is acquired and the file is never written.
    ///
    /// With `--deep`, instead of (or in addition to) the block-store
    /// replay, stream the bounty audit ledger (`--bounty-events`) and
    /// report how many accepted-lean proof events are eligible for
    /// offline Lean re-execution. The actual re-run wiring lands in a
    /// follow-up sub-slice; today every eligible event is reported under
    /// `leanProofsSkipped`.
    Verify {
        /// Path to the durable blocks NDJSON file (typically
        /// `<state-dir>/blocks.ndjson`). Required unless `--deep` is set.
        #[arg(long)]
        blocks: Option<PathBuf>,
        /// Run the P1.4 deep verification pass over the bounty audit
        /// ledger. Requires `--bounty-events`.
        #[arg(long)]
        deep: bool,
        /// Path to the bounty audit ledger NDJSON file (typically
        /// `<state-dir>/bounty-events.ndjson`). Required when `--deep`
        /// is set.
        #[arg(long)]
        bounty_events: Option<PathBuf>,
        /// Lean checker package directory used by the follow-up
        /// sub-slice to re-execute accepted-lean proof events. Accepted
        /// today but unused; supplying it does not yet flip events into
        /// `leanProofsReverified`.
        #[arg(long)]
        lean_checker_dir: Option<PathBuf>,
        /// Emit JSON output.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum BlockCommand {
    /// Fetch the latest block envelope from a node.
    Latest {
        /// Base URL of the boole-node (default http://127.0.0.1:8080).
        #[arg(long)]
        node: Option<String>,
        /// Emit JSON output. Always set on stdout for now.
        #[arg(long)]
        json: bool,
    },
    /// Fetch a block by height from a node.
    Get {
        /// Block height (non-negative integer).
        #[arg(long)]
        height: String,
        /// Base URL of the boole-node (default http://127.0.0.1:8080).
        #[arg(long)]
        node: Option<String>,
        /// Emit JSON output. Always set on stdout for now.
        #[arg(long)]
        json: bool,
    },
}

fn main() {
    let cli = Cli::parse();
    let result = run(cli);
    if let Err(err) = result {
        // Top-level catch-all: any error path that did not already write a
        // typed envelope to stderr lands here. Wrap as `internal_error` so
        // the CLI contract (stderr=typed JSON) holds even for unexpected
        // failures from anyhow-bearing code paths.
        eprintln!(
            "{}",
            serde_json::json!({
                "ok": false,
                "reason": "internal_error",
                "detail": err.to_string()
            })
        );
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> anyhow::Result<()> {
    match cli.command {
        Some(Command::Version { json }) => print_version(json),
        Some(Command::Chain { command }) => match command {
            ChainCommand::Replay { fixture, json } => replay_fixture(&fixture, json),
            ChainCommand::AuditReceipts {
                blocks,
                receipts,
                json,
            } => audit_receipts(&blocks, &receipts, json),
            ChainCommand::SettlementReport {
                blocks,
                receipts,
                export_reputation_events,
                json,
            } => settlement_report(
                &blocks,
                &receipts,
                export_reputation_events.as_deref(),
                json,
            ),
        },
        Some(Command::Node { command }) => match command {
            NodeCommand::Start {
                port,
                data_dir,
                scenario,
                genesis,
                max_requests,
            } => node_start(port, &data_dir, scenario.as_deref(), genesis, max_requests),
        },
        Some(Command::Block { command }) => match command {
            BlockCommand::Latest { node, json: _ } => block_latest(node.as_deref()),
            BlockCommand::Get {
                height,
                node,
                json: _,
            } => block_get(&height, node.as_deref()),
        },
        Some(Command::Account { command }) => match command {
            AccountCommand::Balance { pk, node, json } => {
                account_balance(&pk, node.as_deref(), json)
            }
        },
        Some(Command::Reputation { command }) => match command {
            ReputationCommand::Inspect {
                ledger,
                agent_pk,
                json,
            } => reputation_inspect(&ledger, &agent_pk, json),
        },
        Some(Command::Work { command }) => match command {
            WorkCommand::List { node, json } => work_list(node.as_deref(), json),
            WorkCommand::Get { id, node, json } => work_get(&id, node.as_deref(), json),
        },
        Some(Command::Bounty { command }) => match command {
            BountyCommand::List { node, json } => bounty_list(node.as_deref(), json),
            BountyCommand::Get { id, node, json } => bounty_get(&id, node.as_deref(), json),
            BountyCommand::Submit {
                id,
                proof_hash,
                signing_key,
                envelope,
                node,
                json,
            } => bounty_submit(
                &id,
                &proof_hash,
                &signing_key,
                &envelope,
                node.as_deref(),
                json,
            ),
            BountyCommand::Announce {
                id,
                domain,
                problem_hash,
                verifier_kind,
                verifier_metadata,
                reward,
                deadline,
                ts,
                signing_key,
                node,
                json,
            } => bounty_announce(
                &id,
                &domain,
                &problem_hash,
                &verifier_kind,
                &verifier_metadata,
                &reward,
                deadline,
                ts,
                &signing_key,
                node.as_deref(),
                json,
            ),
            BountyCommand::Status {
                id,
                new_status,
                reason,
                ts,
                signing_key,
                node,
                json,
            } => bounty_status(
                &id,
                &new_status,
                reason.as_deref(),
                ts,
                &signing_key,
                node.as_deref(),
                json,
            ),
        },
        Some(Command::Mine { command }) => boole_miner::cli::run_mine(command),
        Some(Command::State { command }) => match command {
            StateCommand::Verify {
                blocks,
                deep,
                bounty_events,
                lean_checker_dir,
                json,
            } => state_verify_dispatch(
                blocks.as_deref(),
                deep,
                bounty_events.as_deref(),
                lean_checker_dir.as_deref(),
                json,
            ),
        },
        Some(Command::Keys { command }) => match command {
            KeysCommand::New { id, dev, dry_run } => keys_new(&id, dev, dry_run),
            KeysCommand::List => keys_list(),
            KeysCommand::Show { id } => keys_show(&id),
            KeysCommand::Sign { id, payload, json } => keys_sign(&id, &payload, json),
            KeysCommand::Verify {
                pk,
                signature,
                payload,
                json,
            } => keys_verify(&pk, &signature, &payload, json),
            KeysCommand::ExportSecret { id } => keys_export_secret(&id),
        },
        Some(Command::SessionKey { command }) => match command {
            SessionKeyCommand::Create {
                local,
                id,
                owner_id,
                agent_id,
                allowed_routes,
                allowed_family,
                allowed_verifier,
                max_fee,
                daily_fee_cap,
                expiry_height,
            } => session_key_create(
                local,
                &id,
                &owner_id,
                &agent_id,
                &allowed_routes,
                &allowed_family,
                &allowed_verifier,
                &max_fee,
                &daily_fee_cap,
                expiry_height,
            ),
            SessionKeyCommand::Inspect { id } => session_key_inspect(&id),
            SessionKeyCommand::Revoke { local, id } => session_key_revoke(local, &id),
        },
        Some(Command::Signer { command }) => match command {
            SignerCommand::SignWork {
                session_id,
                route,
                family,
                verifier,
                fee,
                request_hash,
                nonce,
                payload,
                json,
            } => signer_sign_work(
                &session_id,
                &route,
                &family,
                &verifier,
                &fee,
                &request_hash,
                &nonce,
                &payload,
                json,
            ),
        },
        None => print_version(false),
    }
}

fn print_version(json: bool) -> anyhow::Result<()> {
    if json {
        println!(
            "{}",
            serde_json::json!({ "ok": true, "name": "boole", "version": env!("CARGO_PKG_VERSION") })
        );
    } else {
        println!("boole {}", env!("CARGO_PKG_VERSION"));
    }
    Ok(())
}

#[derive(Debug, serde::Deserialize)]
struct ReplayFixture {
    blocks: Vec<boole_core::PersistedBlock>,
}

fn state_verify_dispatch(
    blocks_path: Option<&Path>,
    deep: bool,
    bounty_events_path: Option<&Path>,
    lean_checker_dir: Option<&Path>,
    json: bool,
) -> anyhow::Result<()> {
    if deep {
        let events = bounty_events_path.unwrap_or_else(|| {
            emit_typed_error(
                "bad_request",
                2,
                serde_json::json!({
                    "detail": "--deep requires --bounty-events",
                    "field": "bounty-events",
                }),
            );
        });
        return state_verify_deep(events, lean_checker_dir, json);
    }
    let blocks = blocks_path.unwrap_or_else(|| {
        emit_typed_error(
            "bad_request",
            2,
            serde_json::json!({
                "detail": "state verify requires --blocks (or --deep with --bounty-events)",
                "field": "blocks",
            }),
        );
    });
    state_verify(blocks, json)
}

/// P1.4 — `boole state verify --deep --bounty-events <ndjson>`.
/// Streams the bounty audit ledger via `boole_node::deep_verify_bounty_events`
/// and emits a `{ok, eventsScanned, leanProofsAccepted, leanProofsReverified,
/// leanProofsSkipped, divergences}` envelope. Today every accepted-lean
/// proof event is reported under `leanProofsSkipped`; the follow-up
/// sub-slice wires the actual Lean re-execution behind the existing
/// `--lean-checker-dir` flag.
fn state_verify_deep(
    events_path: &Path,
    lean_checker_dir: Option<&Path>,
    json: bool,
) -> anyhow::Result<()> {
    let report = boole_node::deep_verify_bounty_events(events_path, lean_checker_dir)
        .unwrap_or_else(|err| match err {
            boole_node::DeepVerifyError::EventsUnreadable { path, detail } => {
                emit_typed_error(
                    "bounty_events_unreadable",
                    2,
                    serde_json::json!({
                        "bountyEventsPath": path.to_string_lossy(),
                        "detail": detail,
                    }),
                );
            }
            boole_node::DeepVerifyError::LedgerInvalid {
                path,
                line_number,
                detail,
            } => {
                emit_typed_error(
                    "ledger_invalid",
                    3,
                    serde_json::json!({
                        "bountyEventsPath": path.to_string_lossy(),
                        "lineNumber": line_number,
                        "detail": detail,
                    }),
                );
            }
        });
    let divergences: Vec<serde_json::Value> = report
        .divergences
        .iter()
        .map(|d| {
            serde_json::json!({
                "workId": d.work_id,
                "proofHash": d.proof_hash,
                "field": d.field,
                "expected": d.expected,
                "actual": d.actual,
            })
        })
        .collect();
    let envelope = serde_json::json!({
        "ok": divergences.is_empty(),
        "eventsScanned": report.events_scanned,
        "leanProofsAccepted": report.lean_proofs_accepted,
        "leanProofsReverified": report.lean_proofs_reverified,
        "leanProofsSkipped": report.lean_proofs_skipped,
        "divergences": divergences,
        "bountyEventsPath": events_path.to_string_lossy(),
    });
    if !divergences.is_empty() {
        // A divergence here means the recorded `checkerArtifactHash` did
        // not match the re-execution. Mirror the rest-of-CLI contract:
        // operation refused → exit 3 with the report on stderr.
        eprintln!("{envelope}");
        std::process::exit(3);
    }
    if json {
        println!("{envelope}");
    } else {
        println!(
            "ok=true eventsScanned={} leanProofsAccepted={} leanProofsReverified={} leanProofsSkipped={}",
            report.events_scanned,
            report.lean_proofs_accepted,
            report.lean_proofs_reverified,
            report.lean_proofs_skipped,
        );
    }
    Ok(())
}

/// P2.8 — `boole state verify --blocks <ndjson>`. Reuses
/// `FileBlockStore::recover` and `replay_blocks` so the offline check
/// exercises the exact same shape contract the node enforces at boot.
/// No state-dir lock is acquired; the file is opened read-only so this
/// is safe to run against a live node's blocks file.
///
/// P2.5 — failures emit a typed `{ok:false, reason, ...}` envelope on
/// stderr and exit with the rest-of-CLI contract: 2 for operator/usage
/// errors (missing file), 3 for replay/state corruption.
fn state_verify(blocks_path: &Path, json: bool) -> anyhow::Result<()> {
    if !blocks_path.exists() {
        emit_typed_error(
            "blocks_unreadable",
            2,
            serde_json::json!({
                "blocksPath": blocks_path.to_string_lossy(),
                "detail": "blocks file does not exist",
            }),
        );
    }
    let store = boole_node::FileBlockStore::recover(blocks_path).unwrap_or_else(|err| {
        emit_typed_error(
            "replay_mismatch",
            3,
            serde_json::json!({
                "blocksPath": blocks_path.to_string_lossy(),
                "detail": err.to_string(),
            }),
        );
    });
    let replay = boole_core::replay_blocks(store.blocks()).unwrap_or_else(|err| {
        emit_typed_error(
            "replay_mismatch",
            3,
            serde_json::json!({
                "blocksPath": blocks_path.to_string_lossy(),
                "detail": err.to_string(),
            }),
        );
    });
    let block_count = store.size() as u64;
    if json {
        println!(
            "{}",
            serde_json::json!({
                "ok": true,
                "height": replay.height,
                "latestC": replay.latest_c,
                "blockCount": block_count,
                "blocksPath": blocks_path.to_string_lossy(),
            })
        );
    } else {
        println!(
            "ok=true height={} latestC={} blockCount={}",
            replay.height, replay.latest_c, block_count
        );
    }
    Ok(())
}

fn replay_fixture(path: &Path, json: bool) -> anyhow::Result<()> {
    // P2.5 follow-up — distinguish operator typos (exit 2, bad usage)
    // from chain corruption (exit 3, operation refused) so automation
    // can route the failure without parsing free-form anyhow detail.
    let raw = std::fs::read_to_string(path).unwrap_or_else(|err| {
        emit_typed_error(
            "fixture_unreadable",
            2,
            serde_json::json!({
                "fixturePath": path.to_string_lossy(),
                "detail": err.to_string(),
            }),
        );
    });
    let fixture: ReplayFixture = serde_json::from_str(&raw).unwrap_or_else(|err| {
        emit_typed_error(
            "fixture_invalid",
            2,
            serde_json::json!({
                "fixturePath": path.to_string_lossy(),
                "detail": err.to_string(),
            }),
        );
    });
    let replay = boole_core::replay_blocks(&fixture.blocks).unwrap_or_else(|err| {
        emit_typed_error(
            "replay_mismatch",
            3,
            serde_json::json!({
                "fixturePath": path.to_string_lossy(),
                "detail": err.to_string(),
            }),
        );
    });
    if json {
        println!(
            "{}",
            serde_json::json!({
                "ok": true,
                "latestC": replay.latest_c,
                "height": replay.height,
                "balances": replay.balances.into_iter().map(|(pk, amount)| (pk, amount.to_string())).collect::<std::collections::BTreeMap<_, _>>()
            })
        );
    } else {
        println!("latestC={} height={}", replay.latest_c, replay.height);
    }
    Ok(())
}

fn audit_receipts(blocks_path: &Path, receipts_path: &Path, json: bool) -> anyhow::Result<()> {
    // P2.5 follow-up — split operator typos (exit 2, reason="*_unreadable"
    // or "*_invalid") from audit refusal (exit 3, reason="audit_mismatch")
    // so automation does not have to parse free-form anyhow detail.
    let blocks =
        read_ndjson_or_emit_typed_error::<boole_core::PersistedBlock>(blocks_path, "blocks");
    let receipts =
        read_ndjson_or_emit_typed_error::<boole_core::SubmitReceipt>(receipts_path, "receipts");
    let report = boole_core::audit_submit_receipts(&blocks, &receipts).unwrap_or_else(|err| {
        emit_typed_error(
            "audit_mismatch",
            3,
            serde_json::json!({
                "blocksPath": blocks_path.to_string_lossy(),
                "receiptsPath": receipts_path.to_string_lossy(),
                "detail": err.to_string(),
            }),
        );
    });
    if json {
        println!(
            "{}",
            serde_json::to_string(&serde_json::json!({
                "ok": report.ok,
                "auditMode": "shape-only",
                "lineageRequired": false,
                "blocksChecked": report.blocks_checked,
                "receiptsChecked": report.receipts_checked,
                "evidence": report.evidence,
                "settlement": report.settlement,
            }))?
        );
    } else {
        println!(
            "ok={} blocksChecked={} receiptsChecked={}",
            report.ok, report.blocks_checked, report.receipts_checked
        );
    }
    Ok(())
}

fn settlement_report(
    blocks_path: &Path,
    receipts_path: &Path,
    export_reputation_events: Option<&Path>,
    json: bool,
) -> anyhow::Result<()> {
    // P2.5 follow-up — settlement-report shares inputs with audit-receipts;
    // emit the same typed envelope dialect (blocks_unreadable / blocks_invalid
    // / receipts_unreadable / receipts_invalid for usage errors,
    // audit_mismatch for refusal) so both commands fail the same way for
    // the same root cause.
    let blocks =
        read_ndjson_or_emit_typed_error::<boole_core::PersistedBlock>(blocks_path, "blocks");
    let receipts =
        read_ndjson_or_emit_typed_error::<boole_core::SubmitReceipt>(receipts_path, "receipts");
    let report = boole_core::audit_submit_receipts(&blocks, &receipts).unwrap_or_else(|err| {
        emit_typed_error(
            "audit_mismatch",
            3,
            serde_json::json!({
                "blocksPath": blocks_path.to_string_lossy(),
                "receiptsPath": receipts_path.to_string_lossy(),
                "detail": err.to_string(),
            }),
        );
    });
    let reputation_events_exported = if let Some(path) = export_reputation_events {
        export_reputation_event_rows(path, &report.settlement.reputation_deltas)?
    } else {
        0
    };
    if json {
        println!(
            "{}",
            serde_json::to_string(&serde_json::json!({
                "ok": report.ok,
                "source": "audit-receipts-shape-only",
                "auditMode": "shape-only",
                "claimBoundary": "shape-only local audit; no ledger mutation",
                "lineageRequired": false,
                "lineageVerified": false,
                "rewardLedgerMutated": false,
                "reputationLedgerMutated": false,
                "blocksChecked": report.blocks_checked,
                "receiptsChecked": report.receipts_checked,
                "reputationEventsExported": reputation_events_exported,
                "reputationEventsPath": export_reputation_events.map(|path| path.to_string_lossy().to_string()),
                "settlement": report.settlement,
            }))?
        );
    } else {
        println!(
            "ok={} source=audit-receipts-shape-only claimBoundary=shape-only-local-audit-no-ledger-mutation lineageVerified=false rewardLedgerMutated=false reputationLedgerMutated=false rewardCredits={} reputationDeltas={} reputationEventsExported={}",
            report.ok,
            report.settlement.reward_credits.len(),
            report.settlement.reputation_deltas.len(),
            reputation_events_exported
        );
    }
    Ok(())
}

fn export_reputation_event_rows(
    path: &Path,
    deltas: &[boole_core::SubmitReceiptReputationDelta],
) -> anyhow::Result<u64> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::File::create(path)?;
    for delta in deltas {
        let event = serde_json::json!({
            "schema": boole_node::REPUTATION_EVENT_SCHEMA,
            "agentPk": delta.agent_pk,
            "acceptedSubmits": delta.accepted_submits,
            "verifiedRewardAmount": delta.verified_reward_amount,
            "source": "settlement-report-shape-only",
            "lineageVerified": false,
        });
        writeln!(file, "{}", serde_json::to_string(&event)?)?;
    }
    Ok(deltas.len() as u64)
}

/// NDJSON reader that emits typed error envelopes directly via
/// [`emit_typed_error`] (exit 2 with `reason="{resource}_unreadable"`
/// for I/O and `"{resource}_invalid"` for parse errors), so command
/// surfaces using it do not have to translate anyhow into the typed
/// dialect themselves. Used by `chain audit-receipts` and
/// `chain settlement-report` (P2.5).
fn read_ndjson_or_emit_typed_error<T>(path: &Path, resource: &str) -> Vec<T>
where
    T: serde::de::DeserializeOwned,
{
    let raw = std::fs::read_to_string(path).unwrap_or_else(|err| {
        emit_typed_error(
            &format!("{resource}_unreadable"),
            2,
            serde_json::json!({
                "path": path.to_string_lossy(),
                "detail": err.to_string(),
            }),
        );
    });
    raw.lines()
        .enumerate()
        .filter_map(|(idx, line)| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some((idx, trimmed))
            }
        })
        .map(|(idx, line)| {
            serde_json::from_str(line).unwrap_or_else(|err| {
                emit_typed_error(
                    &format!("{resource}_invalid"),
                    2,
                    serde_json::json!({
                        "path": path.to_string_lossy(),
                        "line": idx + 1,
                        "detail": err.to_string(),
                    }),
                );
            })
        })
        .collect()
}

/// Resolve the boole-node binary used by `node start`. Tests set
/// BOOLE_NODE_BIN to point at the workspace target binary; production runs
/// fall back to a sibling of the boole-cli binary (cargo workspace layout)
/// before relying on PATH.
fn resolve_node_binary() -> anyhow::Result<PathBuf> {
    if let Ok(explicit) = std::env::var("BOOLE_NODE_BIN") {
        return Ok(PathBuf::from(explicit));
    }
    let cli_bin = std::env::current_exe()?;
    if let Some(parent) = cli_bin.parent() {
        let sibling = parent.join("boole-node");
        if sibling.exists() {
            return Ok(sibling);
        }
    }
    Ok(PathBuf::from("boole-node"))
}

// P1.10c (follow-up, 2026-05-18 design review) — wipe the parent
// process env before spawning `boole-node` so secrets the operator may
// keep in shell env (LLM API keys, AWS_* tokens, SSH agent sockets,
// x402 payment keys, etc.) cannot leak into the spawned node.
//
// The original P1.10 cut forwarded the entire `BOOLE_*` prefix, but
// that prefix is shared across miner/wallet/signer surfaces. Keys like
// `BOOLE_LLM_API_KEY`, `BOOLE_ALLOW_PAID_LLM`, `BOOLE_KEYS_DIR`,
// `BOOLE_SESSIONS_DIR`, and `BOOLE_SIGNER_NONCE_DIR` are *not* node
// knobs; forwarding them widens the leak surface env_clear was meant
// to close. We instead apply a strict by-name allowlist of node-owned
// env vars (those that `boole-node`'s `RunLocalArgs` reads via clap's
// `env` attribute) plus the POSIX minimum (PATH, HOME, LANG). Adding a
// new node-owned env var becomes an explicit edit to
// `is_node_child_env_allowed`, which is the review gate.
//
// The pure variant takes a parent-env iterator so the policy is
// unit-testable without touching the real process env.
fn is_node_child_env_allowed(key: &str) -> bool {
    matches!(
        key,
        "PATH"
            | "HOME"
            | "LANG"
            | "BOOLE_STATE_DIR"
            | "BOOLE_NETWORK_ID"
            | "BOOLE_SESSION_REGISTRY_PATH"
            | "BOOLE_SUBMIT_NONCE_LEDGER_PATH"
            | "BOOLE_SUBMIT_RECEIPT_LEDGER_PATH"
            | "BOOLE_RECEIPT_COMMITMENT_LEDGER_PATH"
    )
}

fn configure_node_child_environment_from<I, K, V>(
    command: &mut std::process::Command,
    parent_env: I,
) where
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<str>,
    V: AsRef<str>,
{
    command.env_clear();
    let mut had_lang = false;
    for (key, value) in parent_env {
        let key = key.as_ref();
        let value = value.as_ref();
        if !is_node_child_env_allowed(key) {
            continue;
        }
        command.env(key, value);
        if key == "LANG" {
            had_lang = true;
        }
    }
    if !had_lang {
        command.env("LANG", "C.UTF-8");
    }
}

fn configure_node_child_environment(command: &mut std::process::Command) {
    configure_node_child_environment_from(command, std::env::vars());
}

fn node_start(
    port: Option<u16>,
    data_dir: &Path,
    scenario: Option<&Path>,
    genesis: Option<String>,
    max_requests: Option<usize>,
) -> anyhow::Result<()> {
    std::fs::create_dir_all(data_dir)?;
    let block_path = data_dir.join("blocks.ndjson");
    // Anchor the reward ledger inside `data_dir` instead of inheriting the
    // boole-node default at `/tmp/boole-node-rewards.ndjson`. The default is
    // a process-global path; concurrent `node start` invocations (e.g., the
    // boole-cli integration suite) would race on it and the second one would
    // see a stale `ledger=N replay=0` divergence at boot. Per-data-dir
    // isolation makes `node start` self-contained and reproducible.
    let reward_path = data_dir.join("rewards.ndjson");
    let node_bin = resolve_node_binary()?;

    let mut command = std::process::Command::new(&node_bin);
    configure_node_child_environment(&mut command);
    command.arg("run-local");
    if let Some(port) = port {
        command.arg("--port").arg(port.to_string());
    }
    command.arg("--block-store").arg(block_path.as_os_str());
    command.arg("--reward-store").arg(reward_path.as_os_str());
    if let Some(scenario) = scenario {
        command.arg("--scenario").arg(scenario.as_os_str());
    }
    if let Some(genesis) = genesis {
        command.arg("--genesis").arg(genesis);
    }
    if let Some(max) = max_requests {
        command.arg("--max-requests").arg(max.to_string());
    }

    let status = command.status()?;
    if !status.success() {
        anyhow::bail!("boole-node exited with status {status}");
    }
    Ok(())
}

/// Fetch `/account/{pk}/balance` from `node`. Validates `pk` locally first so
/// a malformed input never reaches the wire — matches the typed-rejection
/// shape (`{ok:false, reason:"malformed_pk"}`) the server itself emits, which
/// keeps CLI/server contracts consistent for downstream automation.
fn account_balance(pk: &str, node: Option<&str>, json: bool) -> anyhow::Result<()> {
    if !is_well_formed_hex32(pk) {
        emit_typed_error("malformed_pk", 2, serde_json::json!({ "pk": pk }));
    }
    let url = node.unwrap_or("http://127.0.0.1:8080");
    let path = format!("/account/{pk}/balance");
    let response = http_get(url, &path)?;
    let body_text =
        std::str::from_utf8(&response.body).map_err(|err| anyhow::anyhow!(err.to_string()))?;
    if !(200..300).contains(&response.status) {
        eprintln!("{body_text}");
        std::process::exit(1);
    }
    if json {
        println!("{body_text}");
        return Ok(());
    }
    let parsed: serde_json::Value = serde_json::from_str(body_text)?;
    let balance = parsed
        .get("balance")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("server response missing string balance: {body_text}"))?;
    println!("{balance}");
    Ok(())
}

fn is_well_formed_hex32(s: &str) -> bool {
    boole_core::Hex32::from_hex(s).is_ok()
}

fn reputation_inspect(ledger: &Path, agent_pk: &str, json: bool) -> anyhow::Result<()> {
    if !is_well_formed_hex32(agent_pk) {
        emit_typed_error(
            "malformed-agent-pk",
            2,
            serde_json::json!({ "agentPk": agent_pk }),
        );
    }
    let ledger = boole_node::FileReputationLedger::recover(ledger)?;
    let stats = ledger.stats_for(agent_pk);
    if json {
        println!(
            "{}",
            serde_json::to_string(&serde_json::json!({
                "ok": true,
                "source": "reputation-ledger",
                "ledgerEvents": ledger.size(),
                "stats": stats,
            }))?
        );
    } else {
        println!(
            "agentPk={} acceptedSubmits={} verifiedRewardAmount={} eventCount={}",
            stats.agent_pk, stats.accepted_submits, stats.verified_reward_amount, stats.event_count
        );
    }
    Ok(())
}

/// Fetch `/work` and print one line per manifest by default. Each line is
/// `<workId>\t<familyId>\t<status>` — terse enough to grep, structured enough
/// to feed `column -t` if a human wants a table. `--json` forwards the server
/// envelope verbatim, matching the bare-vs-envelope split used by
/// `account balance`.
fn work_list(node: Option<&str>, json: bool) -> anyhow::Result<()> {
    let url = node.unwrap_or("http://127.0.0.1:8080");
    let response = http_get(url, "/work")?;
    let body_text =
        std::str::from_utf8(&response.body).map_err(|err| anyhow::anyhow!(err.to_string()))?;
    if !(200..300).contains(&response.status) {
        eprintln!("{body_text}");
        std::process::exit(1);
    }
    if json {
        println!("{body_text}");
        return Ok(());
    }
    let parsed: serde_json::Value = serde_json::from_str(body_text)?;
    let work = parsed
        .get("work")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow::anyhow!("server response missing work array: {body_text}"))?;
    for entry in work {
        let work_id = entry
            .get("workId")
            .and_then(|v| v.as_str())
            .unwrap_or("<missing>");
        let family_id = entry
            .get("familyId")
            .and_then(|v| v.as_str())
            .unwrap_or("<missing>");
        let status = entry
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("<missing>");
        println!("{work_id}\t{family_id}\t{status}");
    }
    Ok(())
}

/// Fetch `/work/{id}` and by default print just the embedded `verifierHash`
/// (the obvious useful field for downstream miners). `--json` forwards the
/// envelope; non-2xx (e.g. 404 `work_not_found`) forwards the body to stderr
/// and exits 1, matching `block get` precedent.
fn work_get(id: &str, node: Option<&str>, json: bool) -> anyhow::Result<()> {
    let url = node.unwrap_or("http://127.0.0.1:8080");
    let path = format!("/work/{id}");
    let response = http_get(url, &path)?;
    let body_text =
        std::str::from_utf8(&response.body).map_err(|err| anyhow::anyhow!(err.to_string()))?;
    if !(200..300).contains(&response.status) {
        eprintln!("{body_text}");
        std::process::exit(1);
    }
    if json {
        println!("{body_text}");
        return Ok(());
    }
    let parsed: serde_json::Value = serde_json::from_str(body_text)?;
    let verifier_hash = parsed
        .get("work")
        .and_then(|w| w.get("verifier"))
        .and_then(|v| v.get("metadata"))
        .and_then(|m| m.get("verifierHash"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            anyhow::anyhow!("server response missing verifier.metadata.verifierHash: {body_text}")
        })?;
    println!("{verifier_hash}");
    Ok(())
}

/// Fetch `/bounties` and print one line per bounty by default. Each line
/// is `<id>\t<domain>\t<status>\t<reward>` — adds reward over `work
/// list` because reward is the bounty-specific value miners care about.
/// `--json` forwards the server envelope verbatim.
fn bounty_list(node: Option<&str>, json: bool) -> anyhow::Result<()> {
    let url = node.unwrap_or("http://127.0.0.1:8080");
    let response = http_get(url, "/bounties")?;
    let body_text =
        std::str::from_utf8(&response.body).map_err(|err| anyhow::anyhow!(err.to_string()))?;
    if !(200..300).contains(&response.status) {
        eprintln!("{body_text}");
        std::process::exit(1);
    }
    if json {
        println!("{body_text}");
        return Ok(());
    }
    let parsed: serde_json::Value = serde_json::from_str(body_text)?;
    let bounties = parsed
        .get("bounties")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow::anyhow!("server response missing bounties array: {body_text}"))?;
    for entry in bounties {
        let id = entry
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("<missing>");
        let domain = entry
            .get("domain")
            .and_then(|v| v.as_str())
            .unwrap_or("<missing>");
        let status = entry
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("<missing>");
        let reward = entry
            .get("reward")
            .and_then(|v| v.as_str())
            .unwrap_or("<missing>");
        println!("{id}\t{domain}\t{status}\t{reward}");
    }
    Ok(())
}

/// Fetch `/bounties/{id}` and by default print just the embedded
/// `verifierHash` — same "obvious useful field" choice as `work get`.
/// `--json` forwards the envelope; non-2xx (e.g. 404 `bounty_not_found`)
/// forwards the body to stderr and exits 1.
fn bounty_get(id: &str, node: Option<&str>, json: bool) -> anyhow::Result<()> {
    let url = node.unwrap_or("http://127.0.0.1:8080");
    let path = format!("/bounties/{id}");
    let response = http_get(url, &path)?;
    let body_text =
        std::str::from_utf8(&response.body).map_err(|err| anyhow::anyhow!(err.to_string()))?;
    if !(200..300).contains(&response.status) {
        eprintln!("{body_text}");
        std::process::exit(1);
    }
    if json {
        println!("{body_text}");
        return Ok(());
    }
    let parsed: serde_json::Value = serde_json::from_str(body_text)?;
    let verifier_hash = parsed
        .get("bounty")
        .and_then(|w| w.get("verifier"))
        .and_then(|v| v.get("metadata"))
        .and_then(|m| m.get("verifierHash"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            anyhow::anyhow!("server response missing verifier.metadata.verifierHash: {body_text}")
        })?;
    println!("{verifier_hash}");
    Ok(())
}

/// POST a proof envelope to `/bounties/{id}/proof` and route the response.
/// Default output is the bare bounty status word (`solved`/`open`/`duplicate`)
/// so shell scripts can pipe the result without parsing JSON. `--json`
/// forwards the full server envelope verbatim. Non-2xx forwards the typed
/// envelope (`bounty_not_found`, `bad_proof_hash`, ...) to stderr with exit 1.
fn bounty_submit(
    id: &str,
    proof_hash: &str,
    signing_key: &str,
    envelope: &str,
    node: Option<&str>,
    json: bool,
) -> anyhow::Result<()> {
    if let Err(detail) = validate_key_id(signing_key) {
        emit_typed_error(
            "bad_request",
            2,
            serde_json::json!({ "detail": detail, "field": "signing-key" }),
        );
    }
    let dir = keys_dir();
    let key_path_buf = key_path(&dir, signing_key);
    if !key_path_buf.exists() {
        emit_typed_error(
            "key_not_found",
            3,
            serde_json::json!({ "id": signing_key, "path": key_path_buf.to_string_lossy() }),
        );
    }
    let raw = std::fs::read_to_string(&key_path_buf)?;
    let key_envelope: serde_json::Value = serde_json::from_str(&raw)?;
    let schema = key_envelope
        .get("schema")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if schema != KEYS_SCHEMA_V2 {
        emit_typed_error(
            "legacy_v1_key",
            3,
            serde_json::json!({
                "id": signing_key,
                "schema": schema,
                "detail": "key was created before S13a and has no secret seed; rotate by creating a new key with a different id",
            }),
        );
    }
    let sk_hex = key_envelope
        .get("sk")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("v2 key envelope missing required `sk` field"))?;
    let signing = boole_core::SigningKeyV2::from_seed_hex(sk_hex)
        .map_err(|err| anyhow::anyhow!("stored sk is not a valid ed25519 seed: {err}"))?;

    let envelope_value = read_json_arg(envelope, "envelope")?;
    let url = node.unwrap_or("http://127.0.0.1:8080");
    let path = format!("/bounties/{id}/proof");
    let payload = serde_json::json!({
        "schema": "boole.bounty.proof.v1",
        "bountyId": id,
        "proofHash": proof_hash,
        "prover": signing.pk_hex(),
        "envelope": envelope_value,
        "validBefore": signed_payload_valid_before(),
        "nonce": fresh_signed_envelope_nonce(),
    });
    let signed = signing
        .sign(&payload)
        .map_err(|err| anyhow::anyhow!("ed25519 sign failed: {err}"))?;
    let body = serde_json::json!({
        "schema": signed.schema,
        "payload": signed.payload,
        "pk": signed.pk,
        "signature": signed.signature,
    });
    let response = http_post(url, &path, &body)?;
    let body_text =
        std::str::from_utf8(&response.body).map_err(|err| anyhow::anyhow!(err.to_string()))?;
    if !(200..300).contains(&response.status) {
        eprintln!("{body_text}");
        std::process::exit(1);
    }
    if json {
        println!("{body_text}");
        return Ok(());
    }
    let parsed: serde_json::Value = serde_json::from_str(body_text)?;
    let duplicate = parsed
        .get("duplicate")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if duplicate {
        println!("duplicate");
        return Ok(());
    }
    let status = parsed
        .get("bounty")
        .and_then(|b| b.get("status"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("server response missing bounty.status: {body_text}"))?;
    println!("{status}");
    Ok(())
}

/// Build a `boole.bounty.announce.v1` payload, sign it locally with a
/// stored v2 key, and POST the resulting `boole.signed.v1` envelope to
/// `/bounties`. Local validation runs first so a malformed `--problem-hash`
/// never reaches the wire — matches the typed-rejection precedent set by
/// `account balance`.
#[allow(clippy::too_many_arguments)]
fn bounty_announce(
    id: &str,
    domain: &str,
    problem_hash: &str,
    verifier_kind: &str,
    verifier_metadata: &str,
    reward: &str,
    deadline: u64,
    ts: Option<u64>,
    signing_key: &str,
    node: Option<&str>,
    json: bool,
) -> anyhow::Result<()> {
    if !is_well_formed_hex32(problem_hash) {
        emit_typed_error(
            "malformed-problem-hash",
            2,
            serde_json::json!({
                "problemHash": problem_hash,
                "detail": "expected 64 lowercase hex chars",
            }),
        );
    }
    let metadata = read_json_arg(verifier_metadata, "verifier-metadata")?;
    if !metadata.is_object() {
        emit_typed_error(
            "bad_request",
            2,
            serde_json::json!({
                "detail": "verifier-metadata must be a JSON object",
                "field": "verifier-metadata",
            }),
        );
    }
    if let Err(detail) = validate_key_id(signing_key) {
        emit_typed_error(
            "bad_request",
            2,
            serde_json::json!({ "detail": detail, "field": "signing-key" }),
        );
    }
    let dir = keys_dir();
    let path = key_path(&dir, signing_key);
    if !path.exists() {
        emit_typed_error(
            "key_not_found",
            3,
            serde_json::json!({ "id": signing_key, "path": path.to_string_lossy() }),
        );
    }
    let raw = std::fs::read_to_string(&path)?;
    let key_envelope: serde_json::Value = serde_json::from_str(&raw)?;
    let schema = key_envelope
        .get("schema")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if schema != KEYS_SCHEMA_V2 {
        emit_typed_error(
            "legacy_v1_key",
            3,
            serde_json::json!({
                "id": signing_key,
                "schema": schema,
                "detail": "key was created before S13a and has no secret seed; rotate by creating a new key with a different id",
            }),
        );
    }
    let sk_hex = key_envelope
        .get("sk")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("v2 key envelope missing required `sk` field"))?;
    let signing = boole_core::SigningKeyV2::from_seed_hex(sk_hex)
        .map_err(|err| anyhow::anyhow!("stored sk is not a valid ed25519 seed: {err}"))?;

    let ts_value = ts.unwrap_or_else(unix_ms_now);
    let payload = serde_json::json!({
        "schema": "boole.bounty.announce.v1",
        "id": id,
        "domain": domain,
        "problemHash": problem_hash,
        "verifier": {
            "kind": verifier_kind,
            "metadata": metadata,
        },
        "reward": reward,
        "deadline": deadline,
        "ts": ts_value,
        "validBefore": signed_payload_valid_before(),
        "nonce": fresh_signed_envelope_nonce(),
    });
    let signed = signing
        .sign(&payload)
        .map_err(|err| anyhow::anyhow!("ed25519 sign failed: {err}"))?;
    let envelope = serde_json::json!({
        "schema": signed.schema,
        "payload": signed.payload,
        "pk": signed.pk,
        "signature": signed.signature,
    });

    let url = node.unwrap_or("http://127.0.0.1:8080");
    let response = http_post(url, "/bounties", &envelope)?;
    let body_text =
        std::str::from_utf8(&response.body).map_err(|err| anyhow::anyhow!(err.to_string()))?;
    if !(200..300).contains(&response.status) {
        eprintln!("{body_text}");
        std::process::exit(1);
    }
    if json {
        println!("{body_text}");
        return Ok(());
    }
    let parsed: serde_json::Value = serde_json::from_str(body_text)?;
    let id_str = parsed
        .get("bounty")
        .and_then(|b| b.get("id"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("server response missing bounty.id: {body_text}"))?;
    println!("{id_str}");
    Ok(())
}

fn bounty_status(
    id: &str,
    new_status: &str,
    reason: Option<&str>,
    ts: Option<u64>,
    signing_key: &str,
    node: Option<&str>,
    json: bool,
) -> anyhow::Result<()> {
    if let Err(detail) = validate_key_id(signing_key) {
        emit_typed_error(
            "bad_request",
            2,
            serde_json::json!({ "detail": detail, "field": "signing-key" }),
        );
    }
    let dir = keys_dir();
    let path = key_path(&dir, signing_key);
    if !path.exists() {
        emit_typed_error(
            "key_not_found",
            3,
            serde_json::json!({ "id": signing_key, "path": path.to_string_lossy() }),
        );
    }
    let raw = std::fs::read_to_string(&path)?;
    let key_envelope: serde_json::Value = serde_json::from_str(&raw)?;
    let schema = key_envelope
        .get("schema")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if schema != KEYS_SCHEMA_V2 {
        emit_typed_error(
            "legacy_v1_key",
            3,
            serde_json::json!({
                "id": signing_key,
                "schema": schema,
                "detail": "key was created before S13a and has no secret seed; rotate by creating a new key with a different id",
            }),
        );
    }
    let sk_hex = key_envelope
        .get("sk")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("v2 key envelope missing required `sk` field"))?;
    let signing = boole_core::SigningKeyV2::from_seed_hex(sk_hex)
        .map_err(|err| anyhow::anyhow!("stored sk is not a valid ed25519 seed: {err}"))?;

    let ts_value = ts.unwrap_or_else(unix_ms_now);
    let mut payload = serde_json::Map::new();
    payload.insert(
        "schema".to_string(),
        serde_json::Value::String("boole.bounty.status.v1".to_string()),
    );
    payload.insert("id".to_string(), serde_json::Value::String(id.to_string()));
    payload.insert(
        "newStatus".to_string(),
        serde_json::Value::String(new_status.to_string()),
    );
    if let Some(text) = reason {
        payload.insert(
            "reason".to_string(),
            serde_json::Value::String(text.to_string()),
        );
    }
    payload.insert(
        "ts".to_string(),
        serde_json::Value::Number(serde_json::Number::from(ts_value)),
    );
    payload.insert(
        "validBefore".to_string(),
        serde_json::Value::Number(serde_json::Number::from(signed_payload_valid_before())),
    );
    payload.insert(
        "nonce".to_string(),
        serde_json::Value::String(fresh_signed_envelope_nonce()),
    );
    let payload_value = serde_json::Value::Object(payload);
    let signed = signing
        .sign(&payload_value)
        .map_err(|err| anyhow::anyhow!("ed25519 sign failed: {err}"))?;
    let envelope = serde_json::json!({
        "schema": signed.schema,
        "payload": signed.payload,
        "pk": signed.pk,
        "signature": signed.signature,
    });

    let url = node.unwrap_or("http://127.0.0.1:8080");
    let route = format!("/bounties/{id}/status");
    let response = http_post(url, &route, &envelope)?;
    let body_text =
        std::str::from_utf8(&response.body).map_err(|err| anyhow::anyhow!(err.to_string()))?;
    if !(200..300).contains(&response.status) {
        eprintln!("{body_text}");
        std::process::exit(1);
    }
    if json {
        println!("{body_text}");
        return Ok(());
    }
    let parsed: serde_json::Value = serde_json::from_str(body_text)?;
    let status_str = parsed
        .get("bounty")
        .and_then(|b| b.get("status"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("server response missing bounty.status: {body_text}"))?;
    println!("{status_str}");
    Ok(())
}

fn unix_ms_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// P1.6a — Unix-second deadline stamped onto outgoing signed payloads.
/// 300s window aligns with the node's clock-skew leeway and miner sender,
/// giving operator-visible "submit and walk away" latency without leaving
/// captured envelopes replay-able indefinitely.
const SIGNED_PAYLOAD_VALID_BEFORE_WINDOW_SECS: u64 = 300;

fn signed_payload_valid_before() -> u64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    now.saturating_add(SIGNED_PAYLOAD_VALID_BEFORE_WINDOW_SECS)
}

/// P1.6b — per-envelope nonce stamped into every signed payload. The node
/// persists `(signerPk, nonce)` into the per-signer ledger and rejects
/// replays with 409 `nonce_replayed`. 16 cryptographic bytes from the OS RNG
/// — collision-free across synchronized clocks.
fn fresh_signed_envelope_nonce() -> String {
    use rand_core::{OsRng, RngCore};
    let mut bytes = [0_u8; 16];
    OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// JSON-bearing CLI flags accept either an inline JSON string or a path to
/// a JSON file. Inline takes precedence: if the argument parses as JSON we
/// use it directly; otherwise treat it as a path. This avoids double-quoting
/// pain from shells without giving up the convenience of `--flag @file`
/// patterns when the caller is OK with reading a file.
///
/// `field` is the user-visible name of the flag; it appears in the error
/// detail when neither branch succeeds, so callers should pass the actual
/// flag name (e.g. "envelope", "payload") rather than a generic label.
fn read_json_arg(arg: &str, field: &str) -> anyhow::Result<serde_json::Value> {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(arg) {
        return Ok(value);
    }
    let raw = std::fs::read_to_string(arg).map_err(|err| {
        anyhow::anyhow!("{field} argument is neither inline JSON nor a readable file: {err}")
    })?;
    Ok(serde_json::from_str(&raw)?)
}

fn block_latest(node: Option<&str>) -> anyhow::Result<()> {
    let url = node.unwrap_or("http://127.0.0.1:8080");
    http_get_print(url, "/block/latest")
}

fn block_get(height: &str, node: Option<&str>) -> anyhow::Result<()> {
    let url = node.unwrap_or("http://127.0.0.1:8080");
    let path = format!("/block/{height}");
    http_get_print(url, &path)
}

/// Send an HTTP GET and route the response: 2xx body to stdout, anything
/// else to stderr with a non-zero exit. The server already speaks the typed
/// envelope (`{ok, reason, ...}`), so on errors we forward the body as-is
/// rather than re-wrapping — that keeps the CLI contract identical to a
/// direct curl against the node.
fn http_get_print(base_url: &str, path: &str) -> anyhow::Result<()> {
    let response = http_get(base_url, path)?;
    let body_text =
        std::str::from_utf8(&response.body).map_err(|err| anyhow::anyhow!(err.to_string()))?;
    if (200..300).contains(&response.status) {
        println!("{body_text}");
        Ok(())
    } else {
        eprintln!("{body_text}");
        std::process::exit(1);
    }
}

struct HttpResponse {
    status: u16,
    body: Vec<u8>,
}

fn http_post(base_url: &str, path: &str, body: &serde_json::Value) -> anyhow::Result<HttpResponse> {
    let stripped = base_url
        .strip_prefix("http://")
        .ok_or_else(|| anyhow::anyhow!("only http:// URLs are supported, got {base_url}"))?;
    let (host_port, base_path) = match stripped.find('/') {
        Some(idx) => (&stripped[..idx], &stripped[idx..]),
        None => (stripped, ""),
    };
    let full_path = if base_path.is_empty() {
        path.to_string()
    } else {
        format!("{}{}", base_path.trim_end_matches('/'), path)
    };
    let host_for_header = host_port.to_string();
    let body_str = serde_json::to_string(body)?;
    let mut stream = TcpStream::connect(host_port)?;
    stream.set_read_timeout(Some(std::time::Duration::from_secs(15)))?;
    stream.set_write_timeout(Some(std::time::Duration::from_secs(15)))?;
    let request = format!(
        "POST {full_path} HTTP/1.1\r\nHost: {host_for_header}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body_str}",
        body_str.len()
    );
    stream.write_all(request.as_bytes())?;
    let mut buffer = Vec::new();
    stream.read_to_end(&mut buffer)?;
    parse_http_response(&buffer)
}

fn http_get(base_url: &str, path: &str) -> anyhow::Result<HttpResponse> {
    let stripped = base_url
        .strip_prefix("http://")
        .ok_or_else(|| anyhow::anyhow!("only http:// URLs are supported, got {base_url}"))?;
    let (host_port, base_path) = match stripped.find('/') {
        Some(idx) => (&stripped[..idx], &stripped[idx..]),
        None => (stripped, ""),
    };
    let full_path = if base_path.is_empty() {
        path.to_string()
    } else {
        format!("{}{}", base_path.trim_end_matches('/'), path)
    };
    let host_for_header = host_port.to_string();
    let mut stream = TcpStream::connect(host_port)?;
    stream.set_read_timeout(Some(std::time::Duration::from_secs(15)))?;
    stream.set_write_timeout(Some(std::time::Duration::from_secs(15)))?;
    let request =
        format!("GET {full_path} HTTP/1.1\r\nHost: {host_for_header}\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes())?;
    let mut buffer = Vec::new();
    stream.read_to_end(&mut buffer)?;
    parse_http_response(&buffer)
}

fn parse_http_response(buffer: &[u8]) -> anyhow::Result<HttpResponse> {
    let header_end = find_header_end(buffer)
        .ok_or_else(|| anyhow::anyhow!("HTTP response missing header terminator"))?;
    let header_text = std::str::from_utf8(&buffer[..header_end])?;
    let mut lines = header_text.split("\r\n");
    let status_line = lines
        .next()
        .ok_or_else(|| anyhow::anyhow!("HTTP response missing status line"))?;
    let mut parts = status_line.split_whitespace();
    let _ = parts.next();
    let status: u16 = parts
        .next()
        .ok_or_else(|| anyhow::anyhow!("HTTP response missing status code"))?
        .parse()?;
    let body = buffer[header_end + 4..].to_vec();
    Ok(HttpResponse { status, body })
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|w| w == b"\r\n\r\n")
}

/// Schema tag for keys produced by `keys new` after S13a. v2 carries the
/// ed25519 secret seed (`sk`) alongside the public key so `keys sign` can
/// load and use the key without a separate KMS lookup. v1 envelopes (which
/// stored only `pk`) remain readable by `keys list`/`keys show` but are
/// refused by `keys sign` with `legacy_v1_key`.
const KEYS_SCHEMA_V2: &str = "boole.keys.v2";

/// Resolve the local keys directory. Tests set BOOLE_KEYS_DIR to an isolated
/// tempdir so they never touch the user's real `~/.boole/keys`. Production
/// runs fall back to `$HOME/.boole/keys`. If $HOME is unset (uncommon), we
/// use the working directory as a last resort — better to write to a known
/// location and surface the path in the envelope than to crash.
fn keys_dir() -> PathBuf {
    if let Ok(explicit) = std::env::var("BOOLE_KEYS_DIR") {
        return PathBuf::from(explicit);
    }
    boole_core::paths::boole_home_root().join("keys")
}

fn key_path(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.json"))
}

/// Return Err with a human-readable detail when `id` does not match
/// `[a-zA-Z0-9_-]+`. Path-shape ids (`a/b`, `..`) are explicitly rejected so
/// `key_path` can never escape the keys directory.
fn validate_key_id(id: &str) -> Result<(), String> {
    if id.is_empty() {
        return Err("id must not be empty".to_string());
    }
    if !id
        .bytes()
        .all(|b| matches!(b, b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_' | b'-'))
    {
        return Err(format!("id must match [a-zA-Z0-9_-]+ (got {id:?})"));
    }
    Ok(())
}

/// ISO 8601 UTC timestamp with second precision. Hand-rolled so we don't pull
/// in chrono/time for one format string. Implements Howard Hinnant's
/// civil-from-days algorithm; correct for the full range of i64 days.
fn now_iso8601_utc() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format_iso8601_utc(secs)
}

fn format_iso8601_utc(unix_secs: u64) -> String {
    let days = (unix_secs / 86_400) as i64;
    let remainder = unix_secs % 86_400;
    let hour = remainder / 3600;
    let minute = (remainder % 3600) / 60;
    let second = remainder % 60;
    let (year, month, day) = days_to_civil(days);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

// Howard Hinnant, "chrono-Compatible Low-Level Date Algorithms" — converts
// days-since-1970-01-01 to (year, month, day). Civil-from-days; correct for
// any i64 input the timestamp arithmetic above can produce.
fn days_to_civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

/// Atomic write at mode 0600: tmp file in the same directory → fsync →
/// rename. Same-directory tmp guarantees the rename is atomic on POSIX
/// filesystems (no cross-device move). The mode is set at open time via
/// `OpenOptionsExt::mode` so the file is never world-readable, even
/// transiently.
fn atomic_write_0600(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension(format!("json.tmp.{}", std::process::id()));
    {
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    if let Err(err) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(err.into());
    }
    Ok(())
}

/// Print a typed error envelope (`{ok:false, reason, ...fields}`) to stderr
/// and exit. Mirrors the server-side `boole-node::http_error` envelope shape
/// so CLI-originated and HTTP-forwarded errors look identical to consumers.
/// Never returns; exit_code follows the pof contract (2 = bad usage,
/// 3 = operation refused).
fn emit_typed_error(reason: &str, exit_code: i32, fields: serde_json::Value) -> ! {
    let mut envelope = serde_json::json!({ "ok": false, "reason": reason });
    if let Some(map) = fields.as_object() {
        let obj = envelope.as_object_mut().expect("envelope is an object");
        for (k, v) in map {
            obj.insert(k.clone(), v.clone());
        }
    }
    eprintln!("{envelope}");
    std::process::exit(exit_code);
}

fn keys_new(id: &str, dev: bool, dry_run: bool) -> anyhow::Result<()> {
    if let Err(detail) = validate_key_id(id) {
        emit_typed_error(
            "bad_request",
            2,
            serde_json::json!({ "detail": detail, "field": "id" }),
        );
    }
    let dir = keys_dir();
    let path = key_path(&dir, id);
    if !dry_run && path.exists() {
        emit_typed_error(
            "key_already_exists",
            3,
            serde_json::json!({ "id": id, "path": path.to_string_lossy() }),
        );
    }

    let signing_key = if dev {
        boole_core::SigningKeyV2::from_dev_id(id)
    } else {
        boole_core::SigningKeyV2::from_random()
            .map_err(|err| anyhow::anyhow!("failed to generate ed25519 key: {err}"))?
    };
    let envelope_key = serde_json::json!({
        "id": id,
        "pk": signing_key.pk_hex(),
        "sk": signing_key.sk_seed_hex(),
        "createdAt": now_iso8601_utc(),
        "schema": KEYS_SCHEMA_V2,
    });
    let public_view = public_key_view(&envelope_key);
    let stdout_envelope = if dry_run {
        serde_json::json!({ "ok": true, "key": public_view, "dryRun": true })
    } else {
        let bytes = serde_json::to_vec_pretty(&envelope_key)?;
        atomic_write_0600(&path, &bytes)?;
        serde_json::json!({ "ok": true, "key": public_view, "path": path.to_string_lossy() })
    };
    println!("{stdout_envelope}");
    Ok(())
}

fn keys_list() -> anyhow::Result<()> {
    let dir = keys_dir();
    let mut keys: Vec<serde_json::Value> = Vec::new();
    if dir.is_dir() {
        // Read every `*.json` entry. Anything that doesn't parse as a key
        // envelope surfaces as `internal_error` rather than being silently
        // skipped — a corrupt file in the keys dir is the user's signal that
        // something is wrong, not a thing we paper over.
        let mut entries: Vec<_> = std::fs::read_dir(&dir)?
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .map(|ext| ext == "json")
                    .unwrap_or(false)
            })
            .collect();
        entries.sort_by_key(|e| e.path());
        for entry in entries {
            let path = entry.path();
            let raw = std::fs::read_to_string(&path)?;
            let value: serde_json::Value = serde_json::from_str(&raw)?;
            keys.push(public_key_view(&value));
        }
    }
    println!("{}", serde_json::json!({ "ok": true, "keys": keys }));
    Ok(())
}

fn keys_show(id: &str) -> anyhow::Result<()> {
    if let Err(detail) = validate_key_id(id) {
        emit_typed_error(
            "bad_request",
            2,
            serde_json::json!({ "detail": detail, "field": "id" }),
        );
    }
    let dir = keys_dir();
    let path = key_path(&dir, id);
    if !path.exists() {
        emit_typed_error(
            "key_not_found",
            3,
            serde_json::json!({ "id": id, "path": path.to_string_lossy() }),
        );
    }
    let raw = std::fs::read_to_string(&path)?;
    let value: serde_json::Value = serde_json::from_str(&raw)?;
    println!(
        "{}",
        serde_json::json!({ "ok": true, "key": public_key_view(&value) })
    );
    Ok(())
}

/// Strip `sk` (and any future secret field) from a stored key envelope before
/// printing to stdout. Disk keeps the full envelope so `boole keys sign` can
/// load `sk`; stdout is the prompt-injection / log-upload surface and must
/// stay public-only.
///
/// Whitelists the four public fields shared by `boole.keys.v1` and
/// `boole.keys.v2` envelopes: `id`, `pk`, `createdAt`, `schema`. Missing
/// fields are echoed as JSON `null` so the caller can still detect schema
/// drift (`schema:null` is a clearer signal than a silently-absent key).
fn public_key_view(value: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "id": value.get("id").cloned().unwrap_or(serde_json::Value::Null),
        "pk": value.get("pk").cloned().unwrap_or(serde_json::Value::Null),
        "createdAt": value.get("createdAt").cloned().unwrap_or(serde_json::Value::Null),
        "schema": value.get("schema").cloned().unwrap_or(serde_json::Value::Null),
    })
}

/// **UNSAFE**: print the full stored envelope including the ed25519 secret
/// seed `sk`. The output is wrapped with `"unsafe": true` and a `"warning"`
/// string so any downstream tool (or human reader) sees the secret-export
/// context, not just the bare key. Use for backup / dev workflows only.
///
/// Behaviour mirrors the redacted `keys_show` so missing/invalid keys flow
/// through the same typed-error surface (`bad_request`, `key_not_found`).
/// v1 envelopes lack `sk` on disk; refusing them with `no_secret_to_export`
/// is safer than printing `sk:null` because callers typically pipe `sk`
/// straight into another tool that would silently accept the null.
fn keys_export_secret(id: &str) -> anyhow::Result<()> {
    if let Err(detail) = validate_key_id(id) {
        emit_typed_error(
            "bad_request",
            2,
            serde_json::json!({ "detail": detail, "field": "id" }),
        );
    }
    let dir = keys_dir();
    let path = key_path(&dir, id);
    if !path.exists() {
        emit_typed_error(
            "key_not_found",
            3,
            serde_json::json!({ "id": id, "path": path.to_string_lossy() }),
        );
    }
    let raw = std::fs::read_to_string(&path)?;
    let value: serde_json::Value = serde_json::from_str(&raw)?;
    if value.get("sk").and_then(|v| v.as_str()).is_none() {
        emit_typed_error(
            "no_secret_to_export",
            3,
            serde_json::json!({
                "id": id,
                "schema": value.get("schema").cloned().unwrap_or(serde_json::Value::Null),
                "detail": "stored envelope has no `sk` (likely a pre-S13a v1 key)",
            }),
        );
    }
    println!(
        "{}",
        serde_json::json!({
            "ok": true,
            "unsafe": true,
            "warning": "secret key export: do not paste into prompts, logs, or agent runtimes",
            "key": value,
        })
    );
    Ok(())
}

/// Schema tag for the local agent session-key envelope shipped in W2.1. The
/// envelope is local-only in this slice — node-side session state lands in
/// N1.x of the agent wallet plan.
const SESSION_SCHEMA_V1: &str = "boole.session.v1";

/// Resolve the local sessions directory. Mirrors `keys_dir()`: tests set
/// BOOLE_SESSIONS_DIR to a tempdir; production falls back to
/// `$HOME/.boole/sessions`, then the working directory if $HOME is unset.
fn sessions_dir() -> PathBuf {
    if let Ok(explicit) = std::env::var("BOOLE_SESSIONS_DIR") {
        return PathBuf::from(explicit);
    }
    boole_core::paths::boole_home_root().join("sessions")
}

fn session_path(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.json"))
}

/// Read the `pk` field from a stored key envelope. Used to resolve
/// `--owner-id` / `--agent-id` arguments into the hex32 public keys that go
/// into the SessionState. Validation reuses `validate_key_id` so a path-shape
/// id can never escape the keys directory.
fn read_key_pk(dir: &Path, id: &str, field: &str) -> anyhow::Result<String> {
    if let Err(detail) = validate_key_id(id) {
        emit_typed_error(
            "bad_request",
            2,
            serde_json::json!({ "detail": detail, "field": field }),
        );
    }
    let path = key_path(dir, id);
    if !path.exists() {
        emit_typed_error(
            "key_not_found",
            3,
            serde_json::json!({ "id": id, "field": field, "path": path.to_string_lossy() }),
        );
    }
    let raw = std::fs::read_to_string(&path)?;
    let value: serde_json::Value = serde_json::from_str(&raw)?;
    let pk = value
        .get("pk")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    match pk {
        Some(pk) => Ok(pk),
        None => emit_typed_error(
            "key_missing_pk",
            3,
            serde_json::json!({ "id": id, "field": field, "path": path.to_string_lossy() }),
        ),
    }
}

/// Create a local session-key policy file under `$BOOLE_SESSIONS_DIR/<id>.json`.
/// The session signing key is freshly generated via `SigningKeyV2::from_random`;
/// the on-disk envelope keeps the secret seed (`sessionSk`) so the W3 signer
/// can load it, but stdout carries only public metadata. `--local` is required
/// in this slice — node registration lands in N1.x of the agent wallet plan.
#[allow(clippy::too_many_arguments)]
fn session_key_create(
    local: bool,
    id: &str,
    owner_id: &str,
    agent_id: &str,
    allowed_routes: &[String],
    allowed_family: &str,
    allowed_verifier: &str,
    max_fee: &str,
    daily_fee_cap: &str,
    expiry_height: u64,
) -> anyhow::Result<()> {
    if !local {
        emit_typed_error(
            "bad_request",
            2,
            serde_json::json!({
                "detail": "only --local sessions are supported in this slice",
                "field": "local",
            }),
        );
    }
    if let Err(detail) = validate_key_id(id) {
        emit_typed_error(
            "bad_request",
            2,
            serde_json::json!({ "detail": detail, "field": "id" }),
        );
    }
    if let Err(e) = max_fee.parse::<u128>() {
        emit_typed_error(
            "bad_request",
            2,
            serde_json::json!({ "detail": e.to_string(), "field": "max-fee" }),
        );
    }
    if let Err(e) = daily_fee_cap.parse::<u128>() {
        emit_typed_error(
            "bad_request",
            2,
            serde_json::json!({ "detail": e.to_string(), "field": "daily-fee-cap" }),
        );
    }

    if allowed_routes.is_empty() {
        emit_typed_error(
            "bad_request",
            2,
            serde_json::json!({
                "detail": "at least one --allowed-route is required",
                "field": "allowed-route",
            }),
        );
    }

    let kdir = keys_dir();
    let owner_pk = read_key_pk(&kdir, owner_id, "owner-id")?;
    let agent_pk = read_key_pk(&kdir, agent_id, "agent-id")?;

    let sdir = sessions_dir();
    let path = session_path(&sdir, id);
    if path.exists() {
        emit_typed_error(
            "session_already_exists",
            3,
            serde_json::json!({ "id": id, "path": path.to_string_lossy() }),
        );
    }

    let session_key = boole_core::SigningKeyV2::from_random()
        .map_err(|e| anyhow::anyhow!("session signing key generation failed: {e}"))?;
    let session_pk = session_key.pk_hex();
    let session_sk = session_key.sk_seed_hex();
    let created_at = now_iso8601_utc();

    let disk_envelope = serde_json::json!({
        "id": id,
        "sessionPk": session_pk,
        "sessionSk": session_sk,
        "ownerPk": owner_pk,
        "agentPk": agent_pk,
        "fixedRewardRecipient": owner_pk,
        "allowedRoutes": allowed_routes,
        "allowedFamily": allowed_family,
        "allowedVerifier": allowed_verifier,
        "maxFee": max_fee,
        "dailyFeeCap": daily_fee_cap,
        "activationHeight": 0,
        "expiryHeight": expiry_height,
        "revoked": false,
        "createdAt": created_at,
        "schema": SESSION_SCHEMA_V1,
    });
    let bytes = serde_json::to_vec_pretty(&disk_envelope)?;
    atomic_write_0600(&path, &bytes)?;

    let public_view = session_public_view(&disk_envelope);
    println!(
        "{}",
        serde_json::json!({
            "ok": true,
            "session": public_view,
            "path": path.to_string_lossy(),
        })
    );
    Ok(())
}

/// Strip the secret seed (`sessionSk`) from a session envelope so the
/// remaining object can safely be printed to stdout. The W0 sk-redaction
/// invariant requires every stdout view to drop secret material; centralizing
/// that filter here keeps create/inspect/revoke in lockstep.
fn session_public_view(envelope: &serde_json::Value) -> serde_json::Value {
    let mut view = envelope.clone();
    if let Some(obj) = view.as_object_mut() {
        obj.remove("sessionSk");
    }
    view
}

/// Load the session envelope at `BOOLE_SESSIONS_DIR/<id>.json`. Emits
/// `bad_request` for malformed ids and `session_not_found` when the file is
/// absent so the operator gets a typed exit code rather than a panic.
fn load_session_envelope(id: &str) -> anyhow::Result<(PathBuf, serde_json::Value)> {
    if let Err(detail) = validate_key_id(id) {
        emit_typed_error(
            "bad_request",
            2,
            serde_json::json!({ "detail": detail, "field": "id" }),
        );
    }
    let dir = sessions_dir();
    let path = session_path(&dir, id);
    if !path.exists() {
        emit_typed_error(
            "session_not_found",
            3,
            serde_json::json!({ "id": id, "path": path.to_string_lossy() }),
        );
    }
    let raw = std::fs::read_to_string(&path)?;
    let envelope: serde_json::Value = serde_json::from_str(&raw)?;
    Ok((path, envelope))
}

/// Print the public policy view for an existing local session-key file.
fn session_key_inspect(id: &str) -> anyhow::Result<()> {
    let (path, envelope) = load_session_envelope(id)?;
    let public_view = session_public_view(&envelope);
    println!(
        "{}",
        serde_json::json!({
            "ok": true,
            "session": public_view,
            "path": path.to_string_lossy(),
        })
    );
    Ok(())
}

/// Mark a local session-key file as revoked. Rewrites the envelope in place
/// via `atomic_write_0600` with `revoked: true`.
///
/// Gap G4 — revocation propagation: this only touches the local file. The
/// authoritative on-chain revocation lives on the node and lands in N1.x;
/// until then `MAX_SESSION_LIFETIME_BLOCKS` bounds worst-case exposure. The
/// stdout `note` field surfaces this so the operator does not assume local
/// revoke is final.
fn session_key_revoke(local: bool, id: &str) -> anyhow::Result<()> {
    if !local {
        emit_typed_error(
            "bad_request",
            2,
            serde_json::json!({
                "detail": "only --local revocations are supported in this slice",
                "field": "local",
            }),
        );
    }
    let (path, mut envelope) = load_session_envelope(id)?;
    if let Some(obj) = envelope.as_object_mut() {
        obj.insert("revoked".to_string(), serde_json::Value::Bool(true));
    } else {
        emit_typed_error(
            "session_not_found",
            3,
            serde_json::json!({
                "id": id,
                "path": path.to_string_lossy(),
                "detail": "stored envelope is not a JSON object",
            }),
        );
    }
    let bytes = serde_json::to_vec_pretty(&envelope)?;
    atomic_write_0600(&path, &bytes)?;
    let public_view = session_public_view(&envelope);
    println!(
        "{}",
        serde_json::json!({
            "ok": true,
            "session": public_view,
            "path": path.to_string_lossy(),
            "note": "local revocation; remote revocation pending — call `boole session-key revoke --node URL ...` once N1.x ships",
        })
    );
    Ok(())
}

/// Resolve the per-session nonce ledger directory. Tests point this at a
/// tempdir; production falls back to `$HOME/.boole/signer-nonces`.
fn signer_nonces_dir() -> PathBuf {
    if let Ok(p) = std::env::var("BOOLE_SIGNER_NONCE_DIR") {
        return PathBuf::from(p);
    }
    boole_core::paths::boole_home_root().join("signer-nonces")
}

fn signer_nonce_path(session_id: &str) -> PathBuf {
    signer_nonces_dir().join(format!("{session_id}.txt"))
}

/// Has `nonce` already been used for this session? The file is a flat
/// newline-delimited ledger; one byte per nonce per session keeps recovery
/// trivial and matches the "first MVP" scope from the agent wallet plan.
fn nonce_already_used(path: &Path, nonce: &str) -> anyhow::Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    let raw = std::fs::read_to_string(path)?;
    Ok(raw.lines().any(|line| line.trim() == nonce))
}

/// Append `nonce` to the per-session ledger with 0600 perms. The append
/// happens only after a successful sign so a mid-flight error (bad payload,
/// disk failure) cannot lock the operator out of retrying with the same
/// nonce — replay safety still holds because no signature was emitted.
fn record_nonce(path: &Path, nonce: &str) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)?;
    f.write_all(nonce.as_bytes())?;
    f.write_all(b"\n")?;
    f.sync_all()?;
    Ok(())
}

/// Authorize a `SignerRequest` against the stored session policy, then
/// ed25519-sign the payload with the session secret seed loaded from disk.
///
/// The session envelope is the source of truth for policy: `allowedFamily`,
/// `allowedVerifier`, `maxFee`, `dailyFeeCap`, and `revoked` come from the
/// W2.1 create envelope. `allowedRoutes` is not yet a CLI argument on
/// `session-key create`, so this slice falls back to the W3.1 fixture
/// defaults (`/verify-answer`, `/submit`) when the envelope omits it; a
/// follow-up slice will plumb routes through `session-key create` once
/// N1.x defines the registered set.
#[allow(clippy::too_many_arguments)]
fn signer_sign_work(
    session_id: &str,
    route: &str,
    family: &str,
    verifier: &str,
    fee: &str,
    request_hash: &str,
    nonce: &str,
    payload_arg: &str,
    json: bool,
) -> anyhow::Result<()> {
    let (_path, envelope) = load_session_envelope(session_id)?;

    if envelope
        .get("revoked")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        emit_typed_error(
            "session_revoked",
            3,
            serde_json::json!({ "sessionId": session_id }),
        );
    }

    let allowed_family = envelope
        .get("allowedFamily")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let allowed_verifier = envelope
        .get("allowedVerifier")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let max_fee = envelope
        .get("maxFee")
        .and_then(|v| v.as_str())
        .unwrap_or("0")
        .to_string();
    let daily_fee_cap = envelope
        .get("dailyFeeCap")
        .and_then(|v| v.as_str())
        .unwrap_or("0")
        .to_string();
    let allowed_routes = match envelope.get("allowedRoutes").and_then(|v| v.as_array()) {
        Some(arr) => arr
            .iter()
            .filter_map(|x| x.as_str().map(|s| s.to_string()))
            .collect::<Vec<_>>(),
        None => emit_typed_error(
            "bad_request",
            2,
            serde_json::json!({
                "sessionId": session_id,
                "field": "allowedRoutes",
                "detail": "stored session envelope missing explicit allowedRoutes",
            }),
        ),
    };
    if allowed_routes.is_empty() {
        emit_typed_error(
            "bad_request",
            2,
            serde_json::json!({
                "sessionId": session_id,
                "field": "allowedRoutes",
                "detail": "stored session envelope has no allowed routes",
            }),
        );
    }

    let policy = boole_core::SessionPolicy {
        can_submit_work: true,
        can_pay_verification_fee: true,
        can_withdraw: false,
        can_transfer: false,
        allowed_routes,
        allowed_family_ids: vec![allowed_family],
        allowed_verifier_ids: vec![allowed_verifier],
        max_fee_per_request: max_fee,
        daily_fee_cap,
    };
    let req = boole_core::SignerRequest {
        route: route.to_string(),
        family_id: family.to_string(),
        verifier_id: verifier.to_string(),
        fee: fee.to_string(),
        request_hash: request_hash.to_string(),
        nonce: nonce.to_string(),
    };
    if let Err(err) = policy.authorize(&req) {
        emit_typed_error(
            "policy_denied",
            3,
            serde_json::json!({
                "sessionId": session_id,
                "detail": err.to_string(),
            }),
        );
    }

    let nonce_path = signer_nonce_path(session_id);
    if nonce_already_used(&nonce_path, nonce)? {
        emit_typed_error(
            "nonce_reuse",
            3,
            serde_json::json!({
                "sessionId": session_id,
                "nonce": nonce,
            }),
        );
    }

    let session_sk = envelope
        .get("sessionSk")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("session envelope missing required `sessionSk` field"))?;
    let signing_key = boole_core::SigningKeyV2::from_seed_hex(session_sk)
        .map_err(|err| anyhow::anyhow!("stored sessionSk is not a valid ed25519 seed: {err}"))?;
    let payload = read_json_arg(payload_arg, "payload")?;
    let computed_request_hash = boole_core::canonical_payload_hash_hex(&payload);
    if computed_request_hash != request_hash {
        emit_typed_error(
            "request_hash_mismatch",
            3,
            serde_json::json!({
                "sessionId": session_id,
                "expected": computed_request_hash,
                "provided": request_hash,
            }),
        );
    }
    let work_request_payload = serde_json::json!({
        "schema": "boole.signer.work.v1",
        "route": route,
        "familyId": family,
        "verifierId": verifier,
        "fee": fee,
        "requestHash": request_hash,
        "nonce": nonce,
        "workPayload": payload,
    });
    let signed = signing_key
        .sign(&work_request_payload)
        .map_err(|err| anyhow::anyhow!("ed25519 sign failed: {err}"))?;

    record_nonce(&nonce_path, nonce)?;

    if json {
        println!(
            "{}",
            serde_json::json!({
                "ok": true,
                "envelope": {
                    "schema": signed.schema,
                    "payload": signed.payload,
                    "pk": signed.pk,
                    "signature": signed.signature,
                }
            })
        );
    } else {
        println!("{}", signed.signature);
    }
    Ok(())
}

/// Sign a JSON payload with the v2 key stored at `id`. v1 keys (no `sk`)
/// are refused with `legacy_v1_key` exit 3 — there is no implicit upgrade
/// path because pk rotation is not safe to do for the operator. Default
/// stdout is the bare hex64 ed25519 signature; `--json` emits the full
/// `boole.signed.v1` envelope wrapped in `{ok:true, envelope:...}`.
fn keys_sign(id: &str, payload_arg: &str, json: bool) -> anyhow::Result<()> {
    if let Err(detail) = validate_key_id(id) {
        emit_typed_error(
            "bad_request",
            2,
            serde_json::json!({ "detail": detail, "field": "id" }),
        );
    }
    let dir = keys_dir();
    let path = key_path(&dir, id);
    if !path.exists() {
        emit_typed_error(
            "key_not_found",
            3,
            serde_json::json!({ "id": id, "path": path.to_string_lossy() }),
        );
    }
    let raw = std::fs::read_to_string(&path)?;
    let key_envelope: serde_json::Value = serde_json::from_str(&raw)?;
    let schema = key_envelope
        .get("schema")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if schema != KEYS_SCHEMA_V2 {
        emit_typed_error(
            "legacy_v1_key",
            3,
            serde_json::json!({
                "id": id,
                "schema": schema,
                "detail": "key was created before S13a and has no secret seed; rotate by creating a new key with a different id",
            }),
        );
    }
    let sk_hex = key_envelope
        .get("sk")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("v2 key envelope missing required `sk` field"))?;
    let signing_key = boole_core::SigningKeyV2::from_seed_hex(sk_hex)
        .map_err(|err| anyhow::anyhow!("stored sk is not a valid ed25519 seed: {err}"))?;
    let payload = read_json_arg(payload_arg, "payload")?;
    let signed = signing_key
        .sign(&payload)
        .map_err(|err| anyhow::anyhow!("ed25519 sign failed: {err}"))?;
    if json {
        println!(
            "{}",
            serde_json::json!({
                "ok": true,
                "envelope": {
                    "schema": signed.schema,
                    "payload": signed.payload,
                    "pk": signed.pk,
                    "signature": signed.signature,
                }
            })
        );
    } else {
        println!("{}", signed.signature);
    }
    Ok(())
}

/// Verify a hex64 ed25519 signature against a hex32 public key and a JSON
/// payload. Stateless — never touches the keys directory. Wire-malformed
/// inputs (bad hex shape) emit a typed `bad_pk` / `bad_signature` envelope
/// on stderr with exit 2; cryptographically wrong signatures are NOT errors
/// — they print `invalid` to stdout and exit 0 because verification ran
/// successfully.
fn keys_verify(pk: &str, signature: &str, payload_arg: &str, json: bool) -> anyhow::Result<()> {
    if !is_well_formed_hex32(pk) {
        emit_typed_error(
            "bad_pk",
            2,
            serde_json::json!({
                "detail": "expected 64 lowercase hex chars",
                "pk": pk,
            }),
        );
    }
    if boole_core::Hex64::from_hex(signature).is_err() {
        emit_typed_error(
            "bad_signature",
            2,
            serde_json::json!({
                "detail": "expected 128 lowercase hex chars",
            }),
        );
    }
    let payload = read_json_arg(payload_arg, "payload")?;
    let valid = match boole_core::verify_signature(pk, signature, &payload) {
        Ok(v) => v,
        Err(detail) => {
            // Defensive: shape checks above should have caught wire-malformed
            // hex. If `verify_signature` still rejects (e.g. ed25519 point
            // not on the curve), surface the same `bad_pk` envelope.
            emit_typed_error("bad_pk", 2, serde_json::json!({ "detail": detail }));
        }
    };
    if json {
        println!("{}", serde_json::json!({ "ok": true, "valid": valid }));
    } else if valid {
        println!("valid");
    } else {
        println!("invalid");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::process::Command;

    fn collect_envs(cmd: &Command) -> HashMap<String, Option<String>> {
        cmd.get_envs()
            .map(|(k, v)| {
                (
                    k.to_string_lossy().into_owned(),
                    v.map(|s| s.to_string_lossy().into_owned()),
                )
            })
            .collect()
    }

    // P1.10c — every subprocess spawn in the production CLI must wipe
    // the parent process environment so secrets the operator may keep
    // in shell env (LLM API keys, AWS_* tokens, SSH agent sockets,
    // x402 payment keys) do not leak into a spawned `boole-node`.
    //
    // The earlier slice (commit bc26562) forwarded the entire `BOOLE_*`
    // prefix by-policy. The 2026-05-18 design review flagged that as
    // too broad: `BOOLE_LLM_API_KEY`, `BOOLE_ALLOW_PAID_LLM`,
    // `BOOLE_KEYS_DIR`, `BOOLE_SESSIONS_DIR`, `BOOLE_SIGNER_NONCE_DIR`
    // all sit inside that prefix and belong to the miner/wallet/signer
    // sides, not to a spawned node. Forwarding them widens the
    // secret-leak surface that env_clear was supposed to close.
    //
    // The fix is a strict by-name allowlist of *node-owned* env vars
    // (those that `boole-node`'s `RunLocalArgs` reads via clap's `env`
    // attribute) plus the POSIX minimum (PATH, HOME, LANG). Anything
    // else — including unrecognized `BOOLE_*` keys — is dropped.
    #[test]
    fn configure_node_child_environment_forwards_only_node_owned_env_allowlist() {
        let parent = vec![
            // Non-BOOLE secrets — must never reach the child.
            ("AWS_SECRET_ACCESS_KEY", "leakage-bait"),
            ("SSH_AUTH_SOCK", "/tmp/ssh.sock"),
            ("OPENAI_API_KEY", "sk-leak"),
            // BOOLE_ secrets that belong to miner/wallet/signer surfaces,
            // not to a spawned node.
            ("BOOLE_LLM_API_KEY", "miner-secret"),
            ("BOOLE_ALLOW_PAID_LLM", "1"),
            ("BOOLE_KEYS_DIR", "/op/keys"),
            ("BOOLE_SESSIONS_DIR", "/op/sessions"),
            ("BOOLE_SIGNER_NONCE_DIR", "/op/signer-nonces"),
            // Unrecognized future BOOLE_ key — must also be dropped under
            // strict allowlist so a new env var cannot bypass review.
            ("BOOLE_FUTURE_KNOB", "speculative"),
            // Node-owned env vars — must be forwarded.
            ("BOOLE_NETWORK_ID", "testnet-local"),
            ("BOOLE_STATE_DIR", "/var/boole/state"),
            (
                "BOOLE_SESSION_REGISTRY_PATH",
                "/var/boole/state/sessions.ndjson",
            ),
            (
                "BOOLE_SUBMIT_NONCE_LEDGER_PATH",
                "/var/boole/state/submit-nonces.ndjson",
            ),
            (
                "BOOLE_SUBMIT_RECEIPT_LEDGER_PATH",
                "/var/boole/state/receipts.ndjson",
            ),
            (
                "BOOLE_RECEIPT_COMMITMENT_LEDGER_PATH",
                "/var/boole/state/receipt-commitments.ndjson",
            ),
            // POSIX minimum — must be forwarded.
            ("PATH", "/usr/local/bin:/usr/bin"),
            ("HOME", "/home/operator"),
            ("LANG", "en_US.UTF-8"),
        ];
        let mut cmd = Command::new("/bin/true");
        configure_node_child_environment_from(&mut cmd, parent);
        let envs = collect_envs(&cmd);

        // Non-BOOLE secrets blocked.
        assert!(!envs.contains_key("AWS_SECRET_ACCESS_KEY"));
        assert!(!envs.contains_key("SSH_AUTH_SOCK"));
        assert!(!envs.contains_key("OPENAI_API_KEY"));

        // BOOLE_-prefixed secrets that belong to miner/wallet/signer,
        // not to a spawned node — must also be blocked.
        assert!(
            !envs.contains_key("BOOLE_LLM_API_KEY"),
            "BOOLE_LLM_API_KEY is a miner/LLM driver secret; the spawned \
             node has no use for it and forwarding it widens the leak \
             surface env_clear was supposed to close"
        );
        assert!(
            !envs.contains_key("BOOLE_ALLOW_PAID_LLM"),
            "BOOLE_ALLOW_PAID_LLM is a miner-side opt-in gate; the node \
             does not consult it"
        );
        assert!(
            !envs.contains_key("BOOLE_KEYS_DIR"),
            "BOOLE_KEYS_DIR points at wallet/key material; the node has \
             no business reading it"
        );
        assert!(
            !envs.contains_key("BOOLE_SESSIONS_DIR"),
            "BOOLE_SESSIONS_DIR is wallet-session-side state, not node \
             state"
        );
        assert!(
            !envs.contains_key("BOOLE_SIGNER_NONCE_DIR"),
            "BOOLE_SIGNER_NONCE_DIR is signer-side nonce state, not node \
             state"
        );

        // Strict allowlist: unrecognized BOOLE_ keys are also dropped so
        // a new env var cannot silently bypass review.
        assert!(
            !envs.contains_key("BOOLE_FUTURE_KNOB"),
            "unknown BOOLE_-prefixed env keys must be dropped under \
             strict allowlist; promotion happens via explicit list edit"
        );

        // Node-owned env vars forwarded.
        assert_eq!(
            envs.get("BOOLE_NETWORK_ID").and_then(|v| v.as_deref()),
            Some("testnet-local")
        );
        assert_eq!(
            envs.get("BOOLE_STATE_DIR").and_then(|v| v.as_deref()),
            Some("/var/boole/state")
        );
        assert_eq!(
            envs.get("BOOLE_SESSION_REGISTRY_PATH")
                .and_then(|v| v.as_deref()),
            Some("/var/boole/state/sessions.ndjson")
        );
        assert_eq!(
            envs.get("BOOLE_SUBMIT_NONCE_LEDGER_PATH")
                .and_then(|v| v.as_deref()),
            Some("/var/boole/state/submit-nonces.ndjson")
        );
        assert_eq!(
            envs.get("BOOLE_SUBMIT_RECEIPT_LEDGER_PATH")
                .and_then(|v| v.as_deref()),
            Some("/var/boole/state/receipts.ndjson")
        );
        assert_eq!(
            envs.get("BOOLE_RECEIPT_COMMITMENT_LEDGER_PATH")
                .and_then(|v| v.as_deref()),
            Some("/var/boole/state/receipt-commitments.ndjson")
        );

        // POSIX minimum forwarded.
        assert_eq!(
            envs.get("PATH").and_then(|v| v.as_deref()),
            Some("/usr/local/bin:/usr/bin"),
        );
        assert_eq!(
            envs.get("HOME").and_then(|v| v.as_deref()),
            Some("/home/operator"),
        );
        assert_eq!(
            envs.get("LANG").and_then(|v| v.as_deref()),
            Some("en_US.UTF-8"),
        );
    }

    #[test]
    fn configure_node_child_environment_supplies_lang_default_when_parent_omits_it() {
        let parent: Vec<(&str, &str)> = vec![("PATH", "/usr/bin")];
        let mut cmd = Command::new("/bin/true");
        configure_node_child_environment_from(&mut cmd, parent);
        let envs = collect_envs(&cmd);
        assert_eq!(
            envs.get("LANG").and_then(|v| v.as_deref()),
            Some("C.UTF-8"),
            "LANG must default to C.UTF-8 if the parent did not set it, \
             matching the `configure_child_environment` policy used by \
             boole-miner and boole-lean-runner",
        );
    }

    #[test]
    fn configure_node_child_environment_does_not_set_path_when_parent_omits_it() {
        // PATH absence is a degenerate operator setup; we do not invent
        // a default because the wrong default could mask a missing
        // BOOLE_NODE_BIN sibling. The child's `execvp` will fail loudly
        // instead, which is the right behavior.
        let parent: Vec<(&str, &str)> = vec![];
        let mut cmd = Command::new("/bin/true");
        configure_node_child_environment_from(&mut cmd, parent);
        let envs = collect_envs(&cmd);
        assert!(
            !envs.contains_key("PATH"),
            "PATH is not synthesized when parent omits it; child execvp \
             surfaces the missing PATH as a real failure",
        );
        // Sanity: LANG still defaults so downstream locale-sensitive
        // parsing stays deterministic.
        assert_eq!(envs.get("LANG").and_then(|v| v.as_deref()), Some("C.UTF-8"));
    }

    #[test]
    fn configure_node_child_environment_calls_env_clear_so_real_parent_env_is_invisible() {
        // Smoke: even without a synthesized parent iterator, the helper
        // must call `env_clear` so when the real CLI invokes it in
        // `node_start`, std::process inherits *nothing* by default and
        // only the policy-forwarded keys reach the child.
        let mut cmd = Command::new("/bin/true");
        cmd.env("DOES_NOT_MATTER", "before");
        configure_node_child_environment_from(&mut cmd, std::iter::empty::<(&str, &str)>());
        let envs = collect_envs(&cmd);
        assert!(
            !envs.contains_key("DOES_NOT_MATTER"),
            "env_clear must wipe any prior override on the Command",
        );
    }
}
