//! P2.5 — unified CLI JSON envelope.
//!
//! Every boole CLI command that emits JSON should serialize through the
//! `encode_ok` / `encode_err` helpers in this module so downstream
//! consumers (operator scripts, IDE plugins, the boole-mcp proxy) can
//! parse every command's output with a single schema instead of one
//! per-command bespoke shape.
//!
//! Envelope shape (schema "v1"):
//!
//! ```text
//! success: {"ok": true,  "version": "v1", "command": "<dotted-path>", "result": <any>}
//! failure: {"ok": false, "version": "v1", "command": "<dotted-path>", "error": {"reason": "<kebab>", ...}}
//! ```
//!
//! The top-level `version` key is the envelope schema version, NOT a
//! domain-data field. Command-specific data lives strictly inside
//! `result` (success) or `error` (failure) so a top-level reader never
//! confuses envelope metadata with payload — this matters because some
//! commands (`version`, `keys show`) themselves carry a domain `version`
//! field that would otherwise shadow the envelope's.
//!
//! `COMMAND_INVENTORY` is the canonical record of every leaf CLI command
//! and the JSON behavior it currently exhibits. The matching drift test
//! in `tests/cli_envelope.rs` is the gate that forces this inventory to
//! be updated in lockstep with any new clap subcommand.

use serde::Serialize;
use serde_json::{json, Map, Value};

pub const ENVELOPE_VERSION: &str = "v1";

/// Encode a successful envelope. `result` becomes the `result` field
/// verbatim; pass `serde_json::Value::Null` if the command has no payload.
pub fn encode_ok(command: &str, result: impl Serialize) -> String {
    let result_value = serde_json::to_value(result).unwrap_or(Value::Null);
    let envelope = json!({
        "ok": true,
        "version": ENVELOPE_VERSION,
        "command": command,
        "result": result_value,
    });
    serde_json::to_string(&envelope).expect("envelope serializes")
}

/// Encode a failure envelope. `reason` is a kebab-case machine token
/// (`"missing-arg"`, `"bad-fixture"`, ...). `extras` is folded into the
/// `error` object alongside `reason`; pass `Value::Null` for none.
pub fn encode_err(command: &str, reason: &str, extras: Value) -> String {
    let mut error = Map::new();
    error.insert("reason".to_string(), Value::String(reason.to_string()));
    if let Value::Object(map) = extras {
        for (k, v) in map {
            // `reason` always wins — callers cannot accidentally clobber
            // the canonical reason token through extras.
            if k == "reason" {
                continue;
            }
            error.insert(k, v);
        }
    }
    let envelope = json!({
        "ok": false,
        "version": ENVELOPE_VERSION,
        "command": command,
        "error": Value::Object(error),
    });
    serde_json::to_string(&envelope).expect("envelope serializes")
}

/// What flavour of stdout a CLI command currently emits.
///
/// `Unified` is the post-P2.5 target. Every other variant captures the
/// existing pre-P2.5 surface, with the migration path tracked per
/// command in `COMMAND_INVENTORY`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputKind {
    /// `{ok, version, command, result?, error?}` via `encode_ok`/`encode_err`.
    Unified,
    /// Bespoke `{"ok": ..., ...}` JSON, not yet adopting the unified shape.
    AdHocJson,
    /// Upstream HTTP body forwarded verbatim from the boole-node API.
    RawServerForward,
    /// Human-readable text only (no JSON option available).
    PlainText,
    /// NDJSON event stream (e.g. `mine start`).
    EventStream,
    /// Always emits JSON regardless of any flag (no plain-text mode).
    JsonAlways,
}

#[derive(Debug, Clone, Copy)]
pub struct CommandSurface {
    /// Dotted command path as the user types it, e.g. `&["chain", "replay"]`.
    pub path: &'static [&'static str],
    /// Whether the command accepts a `--json` (or equivalent) toggle.
    pub has_json_flag: bool,
    /// Output kind when `--json` is set (or the only mode if there is no flag).
    pub output_with_json: OutputKind,
    /// Output kind when `--json` is unset.
    pub output_default: OutputKind,
}

/// Canonical CLI inventory captured during the P2.5 audit.
///
/// Update this table in the same commit as any new clap subcommand;
/// `tests/cli_envelope.rs::inventory_covers_known_command_paths` will
/// fail otherwise.
pub const COMMAND_INVENTORY: &[CommandSurface] = &[
    CommandSurface {
        path: &["version"],
        has_json_flag: true,
        output_with_json: OutputKind::Unified,
        output_default: OutputKind::PlainText,
    },
    CommandSurface {
        path: &["chain", "replay"],
        has_json_flag: true,
        output_with_json: OutputKind::AdHocJson,
        output_default: OutputKind::PlainText,
    },
    CommandSurface {
        path: &["chain", "audit-receipts"],
        has_json_flag: true,
        output_with_json: OutputKind::AdHocJson,
        output_default: OutputKind::PlainText,
    },
    CommandSurface {
        path: &["chain", "settlement-report"],
        has_json_flag: true,
        output_with_json: OutputKind::AdHocJson,
        output_default: OutputKind::PlainText,
    },
    CommandSurface {
        path: &["node", "start"],
        has_json_flag: false,
        output_with_json: OutputKind::PlainText,
        output_default: OutputKind::PlainText,
    },
    CommandSurface {
        path: &["block", "latest"],
        has_json_flag: true,
        output_with_json: OutputKind::RawServerForward,
        output_default: OutputKind::RawServerForward,
    },
    CommandSurface {
        path: &["block", "get"],
        has_json_flag: true,
        output_with_json: OutputKind::RawServerForward,
        output_default: OutputKind::RawServerForward,
    },
    CommandSurface {
        path: &["account", "balance"],
        has_json_flag: true,
        output_with_json: OutputKind::RawServerForward,
        output_default: OutputKind::PlainText,
    },
    CommandSurface {
        path: &["reputation", "inspect"],
        has_json_flag: true,
        output_with_json: OutputKind::AdHocJson,
        output_default: OutputKind::PlainText,
    },
    CommandSurface {
        path: &["work", "list"],
        has_json_flag: true,
        output_with_json: OutputKind::RawServerForward,
        output_default: OutputKind::PlainText,
    },
    CommandSurface {
        path: &["work", "get"],
        has_json_flag: true,
        output_with_json: OutputKind::RawServerForward,
        output_default: OutputKind::PlainText,
    },
    CommandSurface {
        path: &["bounty", "list"],
        has_json_flag: true,
        output_with_json: OutputKind::RawServerForward,
        output_default: OutputKind::PlainText,
    },
    CommandSurface {
        path: &["bounty", "get"],
        has_json_flag: true,
        output_with_json: OutputKind::RawServerForward,
        output_default: OutputKind::PlainText,
    },
    CommandSurface {
        path: &["bounty", "submit"],
        has_json_flag: true,
        output_with_json: OutputKind::RawServerForward,
        output_default: OutputKind::PlainText,
    },
    CommandSurface {
        path: &["bounty", "announce"],
        has_json_flag: true,
        output_with_json: OutputKind::RawServerForward,
        output_default: OutputKind::PlainText,
    },
    CommandSurface {
        path: &["bounty", "status"],
        has_json_flag: true,
        output_with_json: OutputKind::RawServerForward,
        output_default: OutputKind::PlainText,
    },
    CommandSurface {
        path: &["keys", "new"],
        has_json_flag: false,
        output_with_json: OutputKind::JsonAlways,
        output_default: OutputKind::JsonAlways,
    },
    CommandSurface {
        path: &["keys", "list"],
        has_json_flag: false,
        output_with_json: OutputKind::JsonAlways,
        output_default: OutputKind::JsonAlways,
    },
    CommandSurface {
        path: &["keys", "show"],
        has_json_flag: false,
        output_with_json: OutputKind::JsonAlways,
        output_default: OutputKind::JsonAlways,
    },
    CommandSurface {
        path: &["keys", "sign"],
        has_json_flag: true,
        output_with_json: OutputKind::Unified,
        output_default: OutputKind::PlainText,
    },
    CommandSurface {
        path: &["keys", "verify"],
        has_json_flag: true,
        output_with_json: OutputKind::Unified,
        output_default: OutputKind::PlainText,
    },
    CommandSurface {
        path: &["keys", "export-secret"],
        has_json_flag: false,
        output_with_json: OutputKind::JsonAlways,
        output_default: OutputKind::JsonAlways,
    },
    CommandSurface {
        path: &["session-key", "create"],
        has_json_flag: false,
        output_with_json: OutputKind::JsonAlways,
        output_default: OutputKind::JsonAlways,
    },
    CommandSurface {
        path: &["session-key", "inspect"],
        has_json_flag: false,
        output_with_json: OutputKind::JsonAlways,
        output_default: OutputKind::JsonAlways,
    },
    CommandSurface {
        path: &["session-key", "revoke"],
        has_json_flag: false,
        output_with_json: OutputKind::JsonAlways,
        output_default: OutputKind::JsonAlways,
    },
    CommandSurface {
        path: &["signer", "sign-work"],
        has_json_flag: true,
        output_with_json: OutputKind::AdHocJson,
        output_default: OutputKind::PlainText,
    },
    CommandSurface {
        path: &["state", "verify"],
        has_json_flag: true,
        output_with_json: OutputKind::AdHocJson,
        output_default: OutputKind::PlainText,
    },
    CommandSurface {
        path: &["mine", "init"],
        has_json_flag: false,
        output_with_json: OutputKind::PlainText,
        output_default: OutputKind::PlainText,
    },
    CommandSurface {
        path: &["mine", "address"],
        has_json_flag: false,
        output_with_json: OutputKind::PlainText,
        output_default: OutputKind::PlainText,
    },
    CommandSurface {
        path: &["mine", "config", "get"],
        has_json_flag: false,
        output_with_json: OutputKind::PlainText,
        output_default: OutputKind::PlainText,
    },
    CommandSurface {
        path: &["mine", "config", "set"],
        has_json_flag: false,
        output_with_json: OutputKind::PlainText,
        output_default: OutputKind::PlainText,
    },
    CommandSurface {
        path: &["mine", "start"],
        has_json_flag: false,
        output_with_json: OutputKind::EventStream,
        output_default: OutputKind::EventStream,
    },
    CommandSurface {
        path: &["mine", "bounty"],
        has_json_flag: false,
        output_with_json: OutputKind::AdHocJson,
        output_default: OutputKind::AdHocJson,
    },
    // P2.9 — `boole wallet ...` façade subcommands. In non-json mode the
    // façade forwards the agent's stdout verbatim (hex pubkey for init/
    // address/migrate, hex signature for sign); in `--json` mode the
    // façade wraps that scalar in the unified envelope.
    CommandSurface {
        path: &["wallet", "init"],
        has_json_flag: true,
        output_with_json: OutputKind::Unified,
        output_default: OutputKind::RawServerForward,
    },
    CommandSurface {
        path: &["wallet", "address"],
        has_json_flag: true,
        output_with_json: OutputKind::Unified,
        output_default: OutputKind::RawServerForward,
    },
    CommandSurface {
        path: &["wallet", "sign"],
        has_json_flag: true,
        output_with_json: OutputKind::Unified,
        output_default: OutputKind::RawServerForward,
    },
    CommandSurface {
        path: &["wallet", "migrate"],
        has_json_flag: true,
        output_with_json: OutputKind::Unified,
        output_default: OutputKind::RawServerForward,
    },
    // P2.10 — `boole faucet claim`. Non-json mode forwards the faucet
    // server's response body verbatim (so a faucet that returns text or
    // bespoke JSON is not mangled); `--json` wraps the parsed response
    // under the unified envelope's `result` field.
    CommandSurface {
        path: &["faucet", "claim"],
        has_json_flag: true,
        output_with_json: OutputKind::Unified,
        output_default: OutputKind::RawServerForward,
    },
];
