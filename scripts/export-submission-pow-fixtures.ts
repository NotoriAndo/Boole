import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { checkSubmissionPow } from "../../pof/dispatcher/src/submissionPow.ts";
import { submissionPowHash } from "../../pof/dispatcher/src/hash.ts";
import type { CalibrationReport } from "../../pof/dispatcher/src/config.ts";

const outPath = resolve("fixtures/protocol/submission-pow/v1.json");

const c = bytes("aa");
const pk = bytes("bb");
const nonceS = bytes("cc");
const canonHash = bytes("dd");
const hash = submissionPowHash(c, pk, nonceS, canonHash);
const hashInt = BigInt("0x" + Buffer.from(hash).toString("hex"));

const baseCfg: CalibrationReport = {
  T_submit: "0x" + "ff".repeat(32),
  T_share: "0x" + "20".repeat(32),
  T_block: "0x" + "01".repeat(32),
  T_ticket: "0x" + "30".repeat(32),
  MinShareScoreMultiplier: 2,
  K_max: 16,
  ShareCapPerPK_Block: 4,
  L: 4096,
  D_max: 8,
  EMAWindow: 32,
  M: 8,
  perIpRateLimitPer60s: 120,
  provenance: "calibration-final",
};

const cases = [
  { name: "accept_high_threshold", cfg: { ...baseCfg, T_submit: "0x" + "ff".repeat(32) } },
  { name: "reject_zero_threshold", cfg: { ...baseCfg, T_submit: "0x0" } },
  { name: "reject_equal_hash", cfg: { ...baseCfg, T_submit: "0x" + hashInt.toString(16) } },
  { name: "accept_hash_plus_one", cfg: { ...baseCfg, T_submit: "0x" + (hashInt + 1n).toString(16) } },
].map((entry) => {
  const result = checkSubmissionPow({ c, pk, nonceS, canonHash }, entry.cfg);
  return {
    name: entry.name,
    input: {
      c: Buffer.from(c).toString("hex"),
      pk: Buffer.from(pk).toString("hex"),
      nonceS: Buffer.from(nonceS).toString("hex"),
      canonHash: Buffer.from(canonHash).toString("hex"),
    },
    cfg: entry.cfg,
    expected: stringifyBigInts(result),
  };
});

const fixture = {
  version: 1,
  source: {
    submissionPow: "legacy-pof/dispatcher/src/submissionPow.ts",
    hash: "legacy-pof/dispatcher/src/hash.ts",
  },
  generatedBy: "scripts/export-submission-pow-fixtures.ts",
  hashHex: Buffer.from(hash).toString("hex"),
  hashInt: hashInt.toString(),
  cases,
};

mkdirSync(dirname(outPath), { recursive: true });
writeFileSync(outPath, `${JSON.stringify(fixture, null, 2)}\n`);
console.log(`wrote ${outPath}`);

function bytes(hexByte: string): Uint8Array {
  return Buffer.from(hexByte.repeat(32), "hex");
}

function stringifyBigInts(value: unknown): unknown {
  if (typeof value === "bigint") return value.toString();
  if (Array.isArray(value)) return value.map(stringifyBigInts);
  if (value && typeof value === "object") {
    return Object.fromEntries(Object.entries(value).map(([k, v]) => [k, stringifyBigInts(v)]));
  }
  return value;
}
