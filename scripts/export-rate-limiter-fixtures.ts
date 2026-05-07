import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { RateLimiter } from "../../pof/dispatcher/src/rateLimiter.ts";
import type { CalibrationReport } from "../../pof/dispatcher/src/config.ts";

const outPath = resolve("fixtures/protocol/rate-limiter/v1.json");

const cfg: CalibrationReport = {
  T_submit: "0x" + "10".repeat(32),
  T_share: "0x" + "20".repeat(32),
  T_block: "0x" + "01".repeat(32),
  T_ticket: "0x" + "30".repeat(32),
  MinShareScoreMultiplier: 2,
  K_max: 16,
  ShareCapPerPK_Block: 4,
  L: 4096,
  D_max: 8,
  EMAWindow: 32,
  M: 2,
  perIpRateLimitPer60s: 2,
  provenance: "calibration-final",
};

let now = 1_000_000;
const limiter = new RateLimiter(cfg, () => now, 60_000);
const pk = "aa".repeat(32);
const c = "bb".repeat(32);
const n1 = "01".repeat(32);
const n2 = "02".repeat(32);
const ip = "203.0.113.7";

const ops: unknown[] = [];
function op(name: string, fn: () => unknown): void {
  try {
    ops.push({ name, now, ok: true, result: fn() });
  } catch (err) {
    ops.push({ name, now, ok: false, error: (err as Error).message });
  }
}

op("check_no_ticket_pk_quota", () => limiter.check({ ip, pk, c }));
op("observe_exact_n1", () => limiter.observeTicket(pk, c, n1));
op("observe_exact_n1_replay", () => limiter.observeTicket(pk, c, n1));
op("has_observed_n1", () => limiter.hasObservedTicket(pk, c, n1));
op("has_observed_n2_before", () => limiter.hasObservedTicket(pk, c, n2));
op("check_allowed_1", () => limiter.check({ ip, pk, c }));
op("check_allowed_2", () => limiter.check({ ip, pk, c }));
op("check_ip_quota_before_pk_quota", () => limiter.check({ ip, pk, c }));
now += 60_000;
op("check_window_boundary_still_ip_quota", () => limiter.check({ ip, pk, c }));
now += 1;
op("check_pk_quota_after_window", () => limiter.check({ ip, pk, c }));
op("observe_exact_n2", () => limiter.observeTicket(pk, c, n2));
op("has_observed_n2_after", () => limiter.hasObservedTicket(pk, c, n2));
op("check_allowed_after_second_ticket", () => limiter.check({ ip, pk, c }));
op("reset", () => { limiter.reset(); return null; });
op("check_after_reset_no_ticket", () => limiter.check({ ip, pk, c }));
op("observe_legacy", () => limiter.observeTicket(pk, c));
op("legacy_has_any_nonce", () => limiter.hasObservedTicket(pk, c, "ff".repeat(32)));
op("legacy_check_allowed_1", () => limiter.check({ ip, pk, c }));
op("legacy_check_allowed_2", () => limiter.check({ ip: "203.0.113.8", pk, c }));
op("legacy_check_pk_quota", () => limiter.check({ ip: "203.0.113.9", pk, c }));

const fixture = {
  version: 1,
  source: { rateLimiter: "legacy-pof/dispatcher/src/rateLimiter.ts" },
  generatedBy: "scripts/export-rate-limiter-fixtures.ts",
  cfg,
  windowMs: 60_000,
  constants: { pk, c, n1, n2, ip },
  operations: ops,
};

mkdirSync(dirname(outPath), { recursive: true });
writeFileSync(outPath, `${JSON.stringify(fixture, null, 2)}\n`);
console.log(`wrote ${outPath}`);
