import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { SharePool, type AcceptedShare } from "../../pof/dispatcher/src/sharePool.ts";

const outPath = resolve("fixtures/protocol/share-pool/v1.json");

const cfg = {
  T_submit: "0x0fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
  T_share: "0x0fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
  T_block: "0x000fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
  T_ticket: "0x0fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
  MinShareScoreMultiplier: 1.0,
  K_max: 1024,
  ShareCapPerPK_Block: 2,
  L: 65536,
  D_max: 64,
  EMAWindow: 300,
  M: 32,
  perIpRateLimitPer60s: 60,
  provenance: "rust-migration-share-pool-fixture",
};

function share(label: string, pk: string, n: string, j: string, c: string): AcceptedShare & { label: string } {
  return {
    label,
    pk,
    n,
    j,
    c,
    canonHash: new Uint8Array(32),
    canonBytes: new Uint8Array([1]),
    shareHash: new Uint8Array(32),
    score: 1n,
    submitterIp: "fixture",
    receivedAt: 0,
  };
}

const currentC = "c0";
const nextC = "c1";
const s1 = share("s1", "aa", "00", "00", currentC);
const s2 = share("s2", "aa", "00", "01", currentC);
const duplicateS1 = share("duplicate-s1", "aa", "00", "00", currentC);
const capExceeded = share("cap-exceeded", "aa", "00", "02", currentC);
const stale = share("stale", "bb", "00", "00", "old");
const next = share("next", "cc", "00", "00", nextC);

const pool = new SharePool(cfg);
pool.setCurrentC(currentC);
const operations: unknown[] = [];
function recordAccept(label: string, sh: AcceptedShare & { label: string }) {
  const result = pool.accept(sh);
  operations.push({ op: "accept", label, result });
}

recordAccept("s1", s1);
recordAccept("s2", s2);
recordAccept("duplicate-s1", duplicateS1);
recordAccept("cap-exceeded", capExceeded);
recordAccept("stale", stale);
operations.push({ op: "forChain", c: currentC, labels: pool.forChain(currentC).map((s) => (s as AcceptedShare & { label: string }).label), size: pool.size() });
const prunedToNext = pool.pruneToHeight(nextC);
operations.push({ op: "pruneToHeight", c: nextC, dropped: prunedToNext, size: pool.size(), labels: pool.forChain(nextC).map((s) => (s as AcceptedShare & { label: string }).label) });
recordAccept("next", next);
operations.push({ op: "forChain", c: nextC, labels: pool.forChain(nextC).map((s) => (s as AcceptedShare & { label: string }).label), size: pool.size() });

const fixture = {
  version: 1,
  source: "legacy-pof/dispatcher/src/sharePool.ts",
  generatedBy: "scripts/export-share-pool-fixtures.ts",
  config: { ShareCapPerPK_Block: cfg.ShareCapPerPK_Block },
  currentC,
  nextC,
  shares: [s1, s2, duplicateS1, capExceeded, stale, next].map((s) => ({
    label: s.label,
    pk: s.pk,
    n: s.n,
    j: s.j,
    c: s.c,
  })),
  operations,
};

mkdirSync(dirname(outPath), { recursive: true });
writeFileSync(outPath, `${JSON.stringify(fixture, null, 2)}\n`);
console.log(`wrote ${outPath}`);
