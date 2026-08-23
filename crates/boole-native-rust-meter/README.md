# Deterministic native Rust meter core (non-activated)

This crate is the first bounded resource-contract core for the Rust tuple-
projection family. It is intentionally **not** connected to the V1 checker,
adapter activation, receipts, blocks or rewards.

The accepted answer body is a deliberately small Rust-shaped language:

- signed `i64` literals, `bool` literals, variables and tuple-field access;
- `let`/`let mut`, assignment, `if` expressions and at most one top-level
  `for <binding> in items` loop;
- `as i64`, `wrapping_add`, `wrapping_sub` and `wrapping_mul` only.

Numeric tuple fields must be converted with `as i64` at the access site before
they can be bound or used in arithmetic. This avoids pretending that the meter
knows an anchor's exact native integer width when the standalone input carries
only normalized signed/unsigned values.

Everything else fails closed before any compiler could be started. In
particular comments and every string/character form are rejected rather than
stripped; loops other than the single bounded `for`, nested loops, recursion,
local items, generics, constants, macros, closures and arbitrary calls are not
part of the language.

## Frozen counter meanings

- `source_bytes`: UTF-8 input bytes.
- `tokens`: lexer tokens (whitespace is not a token).
- `ast_nodes`: program + statement + expression nodes.
- `max_ast_depth`: maximum of structural AST depth and parser nesting depth.
- `operations`: successful semantic let/assign, field, cast, wrapping and
  branch-selection operations.
- `fuel`: one unit for every executed program, statement, expression and loop
  iteration.
- `prefix_items`: item visits across hidden prefixes. One loop over 64 prefixes
  visits exactly `1 + ... + 64 = 2,080` items.

All additions and the prefix-work formula use checked `u64` arithmetic. Limits
are caller-owned, enforced at `value > limit`, and host wall time, RSS, process
count and other telemetry never enter the result. `ResourceUse::canonical_bytes`
binds the seven counters in fixed order as big-endian `u64` values after the
versioned domain prefix. Parser/AST nesting also has a non-overridable safety
ceiling of 256 levels even if a caller asks for more headroom.
