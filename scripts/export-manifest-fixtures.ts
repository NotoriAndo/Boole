import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { parseFamilyManifest, type FamilyManifest } from "/Users/seoyong/projects/pof/dispatcher/src/familyManifest.ts";
import { bountyToWorkManifest } from "/Users/seoyong/projects/pof/dispatcher/src/workManifest.ts";

const outPath = resolve("fixtures/protocol/manifests/v1.json");

const HEX32_A = "ab".repeat(32);
const HEX32_B = "cd".repeat(32);
const HEX32_C = "ef".repeat(32);
const HEX32_D = "01".repeat(32);
const HEX32_E = "23".repeat(32);
const HEX32_F = "45".repeat(32);

const validFamily: FamilyManifest = {
  version: "1",
  familyId: "smart-contract-invariant-v01",
  generatorHash: HEX32_A,
  verifierHash: HEX32_B,
  canonicalizerHash: HEX32_C,
  promptSpecHash: HEX32_D,
  calibrationReportHash: HEX32_E,
  testVectorsHash: HEX32_F,
  resourceLimits: {
    maxProofBytes: 16384,
    verifyTimeoutMs: 30000,
    maxDecls: 1024,
  },
  rewardPolicy: {
    mode: "capped_bonus",
    maxBlockRewardShareBps: 500,
  },
  activationHeight: 123000,
  status: "experimental",
};

const familyCases = [
  { name: "valid", input: validFamily },
  { name: "bad_version", input: { ...validFamily, version: "2" } },
  { name: "bad_hex32", input: { ...validFamily, verifierHash: "CD".repeat(32) } },
  { name: "bad_bps", input: { ...validFamily, rewardPolicy: { mode: "no_protocol_reward", maxBlockRewardShareBps: 1 } } },
  { name: "missing_family_id", input: { ...validFamily, familyId: "" } },
].map((c) => ({ ...c, result: parseFamilyManifest(c.input) }));

const bounty = {
  id: "lean-bounty-1",
  domain: "lean.protocol-invariant",
  problemHash: "99".repeat(32),
  verifier: { kind: "lean", metadata: { verifierHash: HEX32_B, profile: "v02" } },
  reward: "42",
  deadline: 1900000000000,
  status: "open" as const,
  createdAt: 1800000000000,
  updatedAt: 1800000001000,
};

const fixture = {
  version: 1,
  source: {
    familyManifest: "/Users/seoyong/projects/pof/dispatcher/src/familyManifest.ts",
    workManifest: "/Users/seoyong/projects/pof/dispatcher/src/workManifest.ts",
  },
  generatedBy: "scripts/export-manifest-fixtures.ts",
  familyCases,
  workCase: {
    name: "bountyToWorkManifest-open",
    bounty,
    expected: bountyToWorkManifest(bounty),
  },
};

mkdirSync(dirname(outPath), { recursive: true });
writeFileSync(outPath, `${JSON.stringify(fixture, null, 2)}\n`);
console.log(`wrote ${outPath}`);
