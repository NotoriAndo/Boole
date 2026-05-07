import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { _resetConfigCache, hexToBigInt, loadConfig, type CalibrationReport } from "../../pof/dispatcher/src/config.ts";

const outPath = resolve("fixtures/protocol/config/v1.json");

const validReport: CalibrationReport = {
  T_submit: "0x" + "10".repeat(32),
  T_share: "0x" + "20".repeat(32),
  T_block: "0x" + "01".repeat(32),
  T_ticket: "0x" + "30".repeat(32),
  MinShareScoreMultiplier: 2,
  K_max: 16,
  ShareCapPerPK_Block: 4,
  L: 16384,
  D_max: 1024,
  EMAWindow: 32,
  M: 8,
  perIpRateLimitPer60s: 120,
  provenance: "calibration-final",
};

const loadCases = [
  { name: "valid", report: validReport },
  { name: "zero_threshold", report: { ...validReport, T_submit: "0x0" } },
  { name: "over_256", report: { ...validReport, T_submit: "0x1" + "00".repeat(32) } },
  { name: "block_not_less_than_share", report: { ...validReport, T_block: validReport.T_share } },
  { name: "bad_k_max", report: { ...validReport, K_max: 0 } },
  { name: "bad_l", report: { ...validReport, L: 0 } },
  { name: "bad_d_max", report: { ...validReport, D_max: 0 } },
  { name: "bad_multiplier", report: { ...validReport, MinShareScoreMultiplier: 0 } },
];

const tmp = mkdtempSync(join(tmpdir(), "boole-config-fixtures-"));
const cases = loadCases.map(({ name, report }) => {
  const path = join(tmp, `${name}.json`);
  writeFileSync(path, JSON.stringify(report));
  _resetConfigCache();
  try {
    const loaded = loadConfig(path);
    return { name, report, result: { ok: true, loaded } };
  } catch (err) {
    return { name, report, result: { ok: false, error: (err as Error).message } };
  }
});
_resetConfigCache();

const hexCases = ["0x01", "0X10", "ff", "", "0xzz"].map((input) => {
  try {
    return { input, ok: true, value: hexToBigInt(input).toString() };
  } catch (err) {
    return { input, ok: false, error: (err as Error).message };
  }
});

const fixture = {
  version: 1,
  source: {
    config: "legacy-pof/dispatcher/src/config.ts",
  },
  generatedBy: "scripts/export-config-fixtures.ts",
  cases,
  hexCases,
};

mkdirSync(resolve("fixtures/protocol/config"), { recursive: true });
writeFileSync(outPath, `${JSON.stringify(fixture, null, 2)}\n`);
console.log(`wrote ${outPath}`);
