// P2.5 — boole-cli library surface.
//
// The CLI is primarily a bin (`src/main.rs`); this lib module exists so
// reusable, side-effect-free helpers (currently the unified JSON envelope
// + CLI command inventory) are importable from integration tests and
// from sibling crates (e.g. boole-mcp may eventually want to validate
// envelope shape on the proxy side).
pub mod cli_envelope;
