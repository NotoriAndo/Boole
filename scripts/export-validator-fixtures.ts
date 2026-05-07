import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { validate } from "../../pof/dispatcher/src/validator.ts";
import type { CalibrationReport } from "../../pof/dispatcher/src/config.ts";

const outPath = resolve("fixtures/protocol/validator/v1.json");

const baseCfg: CalibrationReport = {
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
  M: 8,
  perIpRateLimitPer60s: 120,
  provenance: "calibration-final",
};

const validEmpty = packageBytes(0);
const validTwoDecls = packageBytes(2);
const trailing = new Uint8Array([...validEmpty, 0x99, 0x88]);
const badMagic = new Uint8Array(validEmpty);
badMagic[0] = 0x00;
const unsupportedVersion = new Uint8Array(validEmpty);
writeU32LEInto(unsupportedVersion, 4, 2);
const unknownExprTag = new Uint8Array(validEmpty);
// offset: magic 4 + version 4 + universeArity 4 + theoremName(empty name u32) 4
unknownExprTag[16] = 0x99;

const cases = [
  { name: "valid_empty", bytes: validEmpty, cfg: baseCfg },
  { name: "valid_two_decls", bytes: validTwoDecls, cfg: baseCfg },
  { name: "too_large_precheck", bytes: validEmpty, cfg: { ...baseCfg, L: validEmpty.length - 1 } },
  { name: "too_many_decls", bytes: validTwoDecls, cfg: { ...baseCfg, D_max: 1 } },
  { name: "decode_bad_magic", bytes: badMagic, cfg: baseCfg },
  { name: "decode_unsupported_version", bytes: unsupportedVersion, cfg: baseCfg },
  { name: "decode_unexpected_eof", bytes: validEmpty.slice(0, 7), cfg: baseCfg },
  { name: "decode_unknown_expr_tag", bytes: unknownExprTag, cfg: baseCfg },
  { name: "decode_trailing_bytes", bytes: trailing, cfg: baseCfg },
].map((c) => ({
  name: c.name,
  bytesHex: Buffer.from(c.bytes).toString("hex"),
  cfg: c.cfg,
  expected: validate(c.bytes, c.cfg),
}));

const fixture = {
  version: 1,
  source: {
    validator: "legacy-pof/dispatcher/src/validator.ts",
    proofPackage: "legacy-pof/dispatcher/src/proofPackage.ts",
  },
  generatedBy: "scripts/export-validator-fixtures.ts",
  cases,
};

mkdirSync(dirname(outPath), { recursive: true });
writeFileSync(outPath, `${JSON.stringify(fixture, null, 2)}\n`);
console.log(`wrote ${outPath}`);

function packageBytes(declCount: number): Uint8Array {
  const out: number[] = [];
  out.push(0x50, 0x4f, 0x46, 0x50); // POFP
  pushU32(out, 1); // version
  pushU32(out, 0); // universeArity
  pushName(out, []); // theoremName
  pushBvar(out, 0); // theoremType
  pushBvar(out, 1); // proofExpr
  pushU32(out, declCount);
  for (let i = 0; i < declCount; i++) {
    pushName(out, [`decl${i}`]);
    pushBvar(out, i);
    pushBvar(out, i + 10);
  }
  return new Uint8Array(out);
}

function pushU32(out: number[], n: number): void {
  out.push(n & 0xff, (n >>> 8) & 0xff, (n >>> 16) & 0xff, (n >>> 24) & 0xff);
}

function writeU32LEInto(bytes: Uint8Array, offset: number, n: number): void {
  bytes[offset] = n & 0xff;
  bytes[offset + 1] = (n >>> 8) & 0xff;
  bytes[offset + 2] = (n >>> 16) & 0xff;
  bytes[offset + 3] = (n >>> 24) & 0xff;
}

function pushName(out: number[], parts: string[]): void {
  pushU32(out, parts.length);
  for (const part of parts) {
    const bytes = Buffer.from(part, "utf8");
    pushU32(out, bytes.length);
    out.push(...bytes);
  }
}

function pushBvar(out: number[], idx: number): void {
  out.push(0x10);
  pushU32(out, idx);
}
