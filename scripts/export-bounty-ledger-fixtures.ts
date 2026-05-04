import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { InMemoryBountyEventLedger, type BountyLedgerEvent } from "/Users/seoyong/projects/pof/dispatcher/src/bountyLedger.ts";

const outPath = resolve("fixtures/protocol/bounty-ledger/v1.json");
const HEX_A = "aa".repeat(32);
const HEX_B = "bb".repeat(32);
const HEX_C = "cc".repeat(32);
const HEX_D = "dd".repeat(32);
const PK_1 = "11".repeat(32);
const PK_2 = "22".repeat(32);

const validEvents: BountyLedgerEvent[] = [
  {
    schemaVersion: 1,
    kind: "create",
    workId: "alpha-1",
    problemHash: HEX_A,
    verifierKind: "lean",
    reward: "7",
    ts: 1800000000000,
  },
  {
    schemaVersion: 1,
    kind: "proof",
    workId: "alpha-1",
    problemHash: HEX_A,
    verifierKind: "lean",
    proofHash: HEX_B,
    solverPk: PK_1,
    accepted: false,
    ts: 1800000000100,
  },
  {
    schemaVersion: 1,
    kind: "proof",
    workId: "alpha-1",
    problemHash: HEX_A,
    verifierKind: "lean",
    proofHash: HEX_C,
    solverPk: PK_2,
    accepted: true,
    reward: "7",
    credit: "7",
    ts: 1800000000200,
  },
  {
    schemaVersion: 1,
    kind: "status_change",
    workId: "beta-1",
    problemHash: HEX_D,
    verifierKind: "wasm",
    prevStatus: "open",
    newStatus: "withdrawn",
    ts: 1800000000300,
  },
];

const invalidCases = [
  { name: "bad_schema", event: { ...validEvents[0], schemaVersion: 2 } },
  { name: "bad_kind", event: { ...validEvents[0], kind: "unknown" } },
  { name: "empty_work_id", event: { ...validEvents[0], workId: "" } },
  { name: "bad_problem_hash", event: { ...validEvents[0], problemHash: HEX_A.toUpperCase() } },
  { name: "empty_verifier_kind", event: { ...validEvents[0], verifierKind: "" } },
  { name: "negative_ts", event: { ...validEvents[0], ts: -1 } },
  { name: "proof_missing_hash", event: omit(validEvents[1], "proofHash") },
  { name: "proof_bad_solver", event: { ...validEvents[1], solverPk: PK_1.toUpperCase() } },
  { name: "proof_missing_accepted", event: omit(validEvents[1], "accepted") },
  { name: "status_missing_prev", event: omit(validEvents[3], "prevStatus") },
  { name: "status_bad_new", event: { ...validEvents[3], newStatus: "closed" } },
];

const ledger = new InMemoryBountyEventLedger();
const appendResults = validEvents.map((event, index) => {
  try {
    ledger.append(event);
    return { index, ok: true, size: ledger.size() };
  } catch (err) {
    return { index, ok: false, error: (err as Error).message };
  }
});

const invalidResults = invalidCases.map(({ name, event }) => {
  const l = new InMemoryBountyEventLedger();
  try {
    l.append(event as BountyLedgerEvent);
    return { name, ok: true };
  } catch (err) {
    return { name, ok: false, error: (err as Error).message };
  }
});

const fixture = {
  version: 1,
  source: {
    bountyLedger: "/Users/seoyong/projects/pof/dispatcher/src/bountyLedger.ts",
  },
  generatedBy: "scripts/export-bounty-ledger-fixtures.ts",
  validEvents,
  appendResults,
  expected: {
    all: ledger.getAll(),
    byWorkIdAlpha: ledger.getByWorkId("alpha-1"),
    bySolverPk1: ledger.getBySolverPk(PK_1),
    byVerifierLean: ledger.getByVerifierKind("lean"),
    byMissing: ledger.getByWorkId("missing"),
    size: ledger.size(),
  },
  invalidCases: invalidCases.map(({ name, event }, i) => ({ name, event, result: invalidResults[i] })),
};

mkdirSync(dirname(outPath), { recursive: true });
writeFileSync(outPath, `${JSON.stringify(fixture, null, 2)}\n`);
console.log(`wrote ${outPath}`);

function omit<T extends Record<string, unknown>>(obj: T, key: keyof T): Record<string, unknown> {
  const out = { ...obj };
  delete out[key];
  return out;
}
