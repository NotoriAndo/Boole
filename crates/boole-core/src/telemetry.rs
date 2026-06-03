//! P0.5 — minimal structured telemetry surface.
//!
//! L8 contract: every Boole binary calls [`init`] from `main` so a single
//! call site reaches the telemetry layer before any other work runs.
//! P0.5 slice 64 (ADR-0004) wires this into a real JSON `tracing`
//! subscriber. [`init`] installs a `tracing_subscriber::fmt().json()`
//! subscriber on stderr whose filter is driven by `RUST_LOG`
//! (default-silent — see [`resolve_directive`]) and whose ANSI styling
//! honours `NO_COLOR`. Later P0.5 slices add request-id propagation,
//! `/metrics` counters, and a panic hook on top of this seam.
//!
//! Boot emission is gated on `BOOLE_TELEMETRY_BOOT=1`. The default is
//! silent so binaries (e.g. `boole-node`) that contract on a clean
//! stderr keep that contract; opt-in surfaces the boot line for
//! operators who want it. The subscriber itself defaults to the `error`
//! level, so installing it does not change the output of any code path
//! that does not opt in via `RUST_LOG`.
//!
//! `BinaryName` is an enum (not a `&str`) so a typo at the call site is a
//! compile error, satisfying the master plan's "typed boundaries" rule.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Once;

use tracing_subscriber::EnvFilter;

/// P0.5 slice 68 — process-wide panic counter. The panic hook installed
/// by [`init`] bumps this and emits a structured final line so a panic is
/// observable in logs and on `/metrics` (`boole_panic_total`) even when
/// the release `panic = "abort"` profile turns the panic into an exit.
/// `pub` so `boole-node`'s `/metrics` renderer can read it across the
/// crate boundary.
pub static PANIC_TOTAL: AtomicUsize = AtomicUsize::new(0);

/// Current count of in-process panics observed by the telemetry hook.
pub fn panic_total() -> usize {
    PANIC_TOTAL.load(Ordering::Relaxed)
}

/// Record one panic: bump the counter and emit a structured error line.
/// Factored out of the hook closure so it is unit-testable without
/// constructing a `PanicHookInfo` or mutating the global hook.
fn note_panic(location: &str, message: &str) {
    PANIC_TOTAL.fetch_add(1, Ordering::Relaxed);
    tracing::error!(
        target: "boole_panic",
        location = location,
        message = message,
        "process panic"
    );
}

/// Extract a human-readable payload from a panic. `panic!("x")` carries a
/// `&str`; `panic!("{}", e)` carries a `String`; anything else is opaque.
fn panic_message(info: &std::panic::PanicHookInfo<'_>) -> String {
    if let Some(s) = info.payload().downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = info.payload().downcast_ref::<String>() {
        s.clone()
    } else {
        "<non-string panic payload>".to_string()
    }
}

/// Install the structured panic hook. Chains the previous hook so the
/// default backtrace/abort behavior is preserved; our hook only adds the
/// counter bump and the structured line on top.
fn install_panic_hook() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let location = info
            .location()
            .map(|l| l.to_string())
            .unwrap_or_else(|| "<unknown>".to_string());
        let message = panic_message(info);
        note_panic(&location, &message);
        prev(info);
    }));
}

/// Identifies the calling binary in startup telemetry so a single log
/// stream multiplexed from several Boole processes stays attributable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryName {
    Node,
    Cli,
    Miner,
    Mcp,
}

impl BinaryName {
    /// Stable on-the-wire identifier; never localized.
    pub fn as_str(self) -> &'static str {
        match self {
            BinaryName::Node => "boole-node",
            BinaryName::Cli => "boole-cli",
            BinaryName::Miner => "boole-miner",
            BinaryName::Mcp => "boole-mcp",
        }
    }
}

static INIT: Once = Once::new();

/// Resolve the `tracing` filter directive from the `RUST_LOG` value.
///
/// Default-silent: a missing, empty, or whitespace-only `RUST_LOG`
/// resolves to `error`, so installing the subscriber does not change the
/// output of any code path that does not opt in. Any non-blank value is
/// passed through verbatim (e.g. `info`, `boole_node=debug,warn`).
fn resolve_directive(rust_log: Option<&str>) -> String {
    match rust_log {
        Some(v) if !v.trim().is_empty() => v.to_string(),
        _ => "error".to_string(),
    }
}

/// ANSI styling is enabled only when `NO_COLOR` is absent
/// (<https://no-color.org/>). JSON output carries no ANSI of its own, but
/// the flag is honoured so the contract holds for any future text
/// formatter and so operators can force-disable colour.
fn ansi_enabled(no_color_set: bool) -> bool {
    !no_color_set
}

/// Run telemetry boot. Idempotent — a second call (e.g. a binary that
/// re-enters `main` under a test harness) is a no-op so the record never
/// doubles.
///
/// Installs a JSON `tracing` subscriber on stderr whose filter is driven
/// by `RUST_LOG` (default-silent — see [`resolve_directive`]) and whose
/// ANSI styling honours `NO_COLOR`. Uses `try_init` so a context that has
/// already set a global subscriber (e.g. a test harness) is not a hard
/// error. The `BOOLE_TELEMETRY_BOOT=1`-gated boot line is unchanged, so
/// the stderr-clean contract that node/cli integration tests assert on
/// still holds when neither env var opts in.
pub fn init(name: BinaryName) {
    INIT.call_once(|| {
        let directive = resolve_directive(std::env::var("RUST_LOG").ok().as_deref());
        let filter = EnvFilter::try_new(&directive).unwrap_or_else(|_| EnvFilter::new("error"));
        let no_color = std::env::var_os("NO_COLOR").is_some();

        // try_init: never panic if a global subscriber is already set.
        let _ = tracing_subscriber::fmt()
            .json()
            .with_ansi(ansi_enabled(no_color))
            .with_writer(std::io::stderr)
            .with_env_filter(filter)
            .try_init();

        // P0.5 slice 68 — structured panic hook + boole_panic_total. Runs
        // in every binary's `init`; chains the previous hook so backtraces
        // and the release `panic = "abort"` exit are preserved.
        install_panic_hook();

        if std::env::var("BOOLE_TELEMETRY_BOOT").as_deref() == Ok("1") {
            eprintln!(
                "boole.telemetry boot binary={} version={} pid={}",
                name.as_str(),
                env!("CARGO_PKG_VERSION"),
                std::process::id()
            );
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_name_strings_are_stable() {
        assert_eq!(BinaryName::Node.as_str(), "boole-node");
        assert_eq!(BinaryName::Cli.as_str(), "boole-cli");
        assert_eq!(BinaryName::Miner.as_str(), "boole-miner");
        assert_eq!(BinaryName::Mcp.as_str(), "boole-mcp");
    }

    #[test]
    fn init_is_idempotent_and_does_not_panic() {
        init(BinaryName::Node);
        init(BinaryName::Node);
    }

    // --- P0.5 slice 68: panic counter ---

    #[test]
    fn note_panic_increments_total() {
        let before = panic_total();
        note_panic("src/foo.rs:1:1", "boom");
        let after = panic_total();
        assert_eq!(
            after,
            before + 1,
            "note_panic must bump boole_panic_total by exactly one"
        );
    }

    #[test]
    fn panic_message_extracts_str_and_string_payloads() {
        // &str payload (panic!("literal")).
        let s: Box<dyn std::any::Any + Send> = Box::new("literal-panic");
        // String payload (panic!("{}", x)).
        let owned: Box<dyn std::any::Any + Send> = Box::new(String::from("owned-panic"));
        // We cannot easily synthesize a PanicHookInfo, so assert the
        // downcast logic directly mirrors panic_message's branches.
        assert_eq!(
            s.downcast_ref::<&str>().copied(),
            Some("literal-panic"),
            "str payloads must downcast to &str"
        );
        assert_eq!(
            owned.downcast_ref::<String>().map(String::as_str),
            Some("owned-panic"),
            "format-arg payloads must downcast to String"
        );
    }

    // --- P0.5 slice 64: subscriber directive / colour resolution ---

    #[test]
    fn directive_defaults_to_error_when_rust_log_absent() {
        // Default-silent: with RUST_LOG unset the subscriber emits only
        // `error`, preserving the stderr-clean contract that CLI UX and
        // integration tests rely on.
        assert_eq!(resolve_directive(None), "error");
    }

    #[test]
    fn directive_defaults_to_error_when_rust_log_blank() {
        // Empty / whitespace-only RUST_LOG is treated as unset.
        assert_eq!(resolve_directive(Some("")), "error");
        assert_eq!(resolve_directive(Some("   ")), "error");
    }

    #[test]
    fn directive_honours_explicit_rust_log() {
        // An operator who opts in gets exactly their directive.
        assert_eq!(resolve_directive(Some("info")), "info");
        assert_eq!(
            resolve_directive(Some("boole_node=debug,warn")),
            "boole_node=debug,warn"
        );
    }

    #[test]
    fn ansi_follows_no_color() {
        // NO_COLOR (https://no-color.org/) must suppress ANSI styling.
        assert!(!ansi_enabled(true));
        assert!(ansi_enabled(false));
    }

    #[test]
    fn subscriber_emits_parseable_json_event() {
        use std::io::Write;
        use std::sync::{Arc, Mutex};

        use tracing_subscriber::fmt::MakeWriter;

        #[derive(Clone)]
        struct BufWriter(Arc<Mutex<Vec<u8>>>);
        impl Write for BufWriter {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(buf);
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        impl<'a> MakeWriter<'a> for BufWriter {
            type Writer = BufWriter;
            fn make_writer(&'a self) -> Self::Writer {
                self.clone()
            }
        }

        let buf = Arc::new(Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::fmt()
            .json()
            .with_ansi(ansi_enabled(false))
            .with_writer(BufWriter(buf.clone()))
            .with_env_filter(EnvFilter::new(resolve_directive(Some("info"))))
            .finish();

        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(target: "boole_test", "telemetry_json_smoke");
        });

        let out = String::from_utf8(buf.lock().unwrap().clone()).unwrap();
        let line = out.lines().next().expect("expected one json line");
        let v: serde_json::Value = serde_json::from_str(line).expect("event must be valid json");
        assert_eq!(v["level"], "INFO");
        assert_eq!(v["fields"]["message"], "telemetry_json_smoke");
    }
}
