import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { InMemoryBountyRegistry } from "/Users/seoyong/projects/pof/dispatcher/src/bountyRegistry.ts";

const outPath = resolve("fixtures/protocol/bounty-registry/v1.json");

const HEX_A = "aa".repeat(32);
const HEX_B = "bb".repeat(32);
const HEX_C = "cc".repeat(32);
const HEX_D = "dd".repeat(32);
const HEX_E = "ee".repeat(32);
const HEX_F = "ff".repeat(32);
const HEX_1 = "11".repeat(32);
const HEX_2 = "22".repeat(32);

const registry = new InMemoryBountyRegistry();

const operations: unknown[] = [];

function capture(name: string, fn: () => unknown): void {
  try {
    operations.push({ name, ok: true, result: fn() });
  } catch (err) {
    operations.push({ name, ok: false, error: (err as Error).message });
  }
}

const createAlpha = {
  id: "alpha-1",
  domain: "lean.protocol-invariant",
  problemHash: HEX_A,
  verifier: { kind: "lean", metadata: { verifierHash: HEX_B, profile: "v1", maxSteps: 4096 } },
  reward: 7n,
  deadline: 1900000000000,
  ts: 1800000000000,
};

const createBeta = {
  id: "beta-1",
  domain: "code.spec-template",
  problemHash: HEX_C,
  verifier: { kind: "wasm", metadata: { verifierHash: HEX_D, template: "parser-roundtrip.v01" } },
  reward: 11n,
  deadline: 1900000005000,
  ts: 1800000000500,
};

const createGamma = {
  id: "gamma-1",
  domain: "lean.protocol-invariant",
  problemHash: HEX_E,
  verifier: { kind: "lean", metadata: { verifierHash: HEX_F, profile: "v2" } },
  reward: 13n,
  deadline: 1900000001000,
  ts: 1800000000600,
};

capture("create_alpha", () => registry.create(createAlpha));
capture("duplicate_create_alpha", () => registry.create(createAlpha));
capture("bad_id_whitespace", () => registry.create({ ...createAlpha, id: "bad id" }));
capture("bad_problem_hash_uppercase", () => registry.create({ ...createAlpha, id: "bad-hash", problemHash: HEX_A.toUpperCase() }));
capture("bad_reward_zero", () => registry.create({ ...createAlpha, id: "zero-reward", reward: 0n }));
capture("create_beta", () => registry.create(createBeta));
capture("create_gamma", () => registry.create(createGamma));
capture("list_open_initial", () => registry.listOpen());
capture("reject_proof_alpha", () => registry.submitProof({ bountyId: "alpha-1", proofHash: HEX_1, prover: HEX_2, accepted: false, ts: 1800000001000 }));
capture("has_rejected_proof_alpha", () => registry.hasProof("alpha-1", HEX_1));
capture("duplicate_rejected_proof_alpha", () => registry.submitProof({ bountyId: "alpha-1", proofHash: HEX_1, prover: HEX_2, accepted: true, ts: 1800000001100 }));
capture("accept_proof_alpha", () => registry.submitProof({ bountyId: "alpha-1", proofHash: HEX_2, prover: HEX_1, accepted: true, ts: 1800000001200 }));
capture("duplicate_accepted_proof_alpha_terminal", () => registry.submitProof({ bountyId: "alpha-1", proofHash: HEX_2, prover: HEX_1, accepted: true, ts: 1800000001300 }));
capture("new_proof_terminal_alpha", () => registry.submitProof({ bountyId: "alpha-1", proofHash: HEX_3(), prover: HEX_1, accepted: false, ts: 1800000001400 }));
capture("withdraw_beta", () => registry.updateStatus({ id: "beta-1", status: "withdrawn", ts: 1800000001500 }));
capture("terminal_transition_beta", () => registry.updateStatus({ id: "beta-1", status: "open", ts: 1800000001600 }));
capture("list_open_final", () => registry.listOpen());
capture("size", () => registry.size());
capture("get_alpha", () => registry.get("alpha-1"));
capture("get_missing", () => registry.get("missing") ?? null);

function HEX_3(): string {
  return "33".repeat(32);
}

const eventLog = [
  { kind: "create", bounty: operations.find((o: any) => o.name === "create_alpha")?.result },
  { kind: "create", bounty: operations.find((o: any) => o.name === "create_beta")?.result },
  { kind: "proof", bountyId: "alpha-1", proofHash: HEX_1, prover: HEX_2, accepted: false, ts: 1800000001000 },
  { kind: "proof", bountyId: "alpha-1", proofHash: HEX_2, prover: HEX_1, accepted: true, ts: 1800000001200 },
  { kind: "status", id: "beta-1", status: "withdrawn", ts: 1800000001500 },
];

const recovery = new InMemoryBountyRegistry();
recovery.create({ ...createAlpha });
recovery.create({ ...createBeta });
recovery.submitProof({ bountyId: "alpha-1", proofHash: HEX_1, prover: HEX_2, accepted: false, ts: 1800000001000 });
recovery.submitProof({ bountyId: "alpha-1", proofHash: HEX_2, prover: HEX_1, accepted: true, ts: 1800000001200 });
recovery.updateStatus({ id: "beta-1", status: "withdrawn", ts: 1800000001500 });

const fixture = {
  version: 1,
  source: {
    bountyRegistry: "/Users/seoyong/projects/pof/dispatcher/src/bountyRegistry.ts",
  },
  generatedBy: "scripts/export-bounty-registry-fixtures.ts",
  constants: { HEX_A, HEX_B, HEX_C, HEX_D, HEX_E, HEX_F, HEX_1, HEX_2, HEX_3: HEX_3() },
  operations,
  eventLog,
  recoveryExpected: {
    list: recovery.list(),
    listOpen: recovery.listOpen(),
    size: recovery.size(),
    hasRejectedProofAlpha: recovery.hasProof("alpha-1", HEX_1),
    hasAcceptedProofAlpha: recovery.hasProof("alpha-1", HEX_2),
  },
};

mkdirSync(dirname(outPath), { recursive: true });
writeFileSync(outPath, `${JSON.stringify(fixture, null, 2)}\n`);
console.log(`wrote ${outPath}`);
