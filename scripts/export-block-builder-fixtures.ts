import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { buildBlock } from "/Users/seoyong/projects/pof/dispatcher/src/blockBuilder.ts";
import { hexToBigInt } from "/Users/seoyong/projects/pof/dispatcher/src/config.ts";
import { fromHex, minShareScore, toHex } from "/Users/seoyong/projects/pof/dispatcher/src/hash.ts";
import type { AcceptedShare } from "/Users/seoyong/projects/pof/dispatcher/src/sharePool.ts";
import type { BooleCheckChecker, BooleCheckRequest, BooleCheckResult } from "/Users/seoyong/projects/pof/dispatcher/src/booleCheck.ts";

const outPath = resolve("fixtures/protocol/block-builder/v1.json");

const cfg = {
  T_submit: "0x0fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
  T_share: "0x0fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
  T_block: "0x000fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
  T_ticket: "0x0fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
  MinShareScoreMultiplier: 1.0,
  K_max: 2,
  ShareCapPerPK_Block: 16,
  L: 65536,
  D_max: 64,
  EMAWindow: 300,
  M: 32,
  perIpRateLimitPer60s: 60,
  provenance: "rust-migration-block-builder-fixture",
};

const minScore = minShareScore(hexToBigInt(cfg.T_share), cfg.MinShareScoreMultiplier);

function share(input: {
  label: string;
  pk: string;
  n: string;
  j: string;
  c?: string;
  shareHash: string;
  score: bigint;
  canonTag: number;
}): AcceptedShare & { label: string } {
  return {
    label: input.label,
    pk: input.pk,
    n: input.n,
    j: input.j,
    c: input.c ?? "c0",
    canonHash: new Uint8Array(32),
    canonBytes: new Uint8Array([input.canonTag]),
    shareHash: fromHex(input.shareHash),
    score: input.score,
    submitterIp: "fixture",
    receivedAt: 0,
  };
}

class AcceptTagsChecker implements BooleCheckChecker {
  constructor(private readonly acceptedTags: Set<number>) {}
  async check(req: BooleCheckRequest): Promise<BooleCheckResult> {
    const tag = req.canonBytes[0] ?? -1;
    const accepted = this.acceptedTags.has(tag);
    return {
      accepted,
      exitCode: accepted ? 0 : 7,
      signal: null,
      wallClockMs: 0,
      timedOut: false,
      stderrSnippet: accepted ? "" : "tag mismatch",
    };
  }
}

const inputShares = [
  // Top proposer by score; accepted; survives K_max.
  share({
    label: "top-proposer",
    pk: "20",
    n: "00",
    j: "00",
    shareHash: "0001000000000000000000000000000000000000000000000000000000000000",
    score: minScore + 300n,
    canonTag: 1,
  }),
  // Second by score; accepted; survives K_max.
  share({
    label: "second-accepted",
    pk: "30",
    n: "00",
    j: "00",
    shareHash: "ff00000000000000000000000000000000000000000000000000000000000000",
    score: minScore + 200n,
    canonTag: 2,
  }),
  // Below K_max despite lexicographically smaller pk; must not be checked.
  share({
    label: "below-kmax",
    pk: "00",
    n: "00",
    j: "00",
    shareHash: "ff00000000000000000000000000000000000000000000000000000000000000",
    score: minScore + 100n,
    canonTag: 3,
  }),
  // Below min score; must not be checked.
  share({
    label: "below-min-score",
    pk: "10",
    n: "00",
    j: "00",
    shareHash: "f000000000000000000000000000000000000000000000000000000000000000",
    score: minScore - 1n,
    canonTag: 4,
  }),
  // Stale c; ignored and not counted as below min score.
  share({
    label: "stale-c",
    pk: "40",
    n: "00",
    j: "00",
    c: "stale",
    shareHash: "0100000000000000000000000000000000000000000000000000000000000000",
    score: minScore + 999n,
    canonTag: 5,
  }),
];

async function main(): Promise<void> {
  const result = await buildBlock("c0", inputShares, cfg, new AcceptTagsChecker(new Set([1, 2])));
  if (!result.ok) throw new Error(`expected ok, got ${JSON.stringify(result)}`);

  const fixture = {
    version: 1,
    source: "/Users/seoyong/projects/pof/dispatcher/src/blockBuilder.ts",
    generatedBy: "scripts/export-block-builder-fixtures.ts",
    chainHead: "c0",
    config: {
      ...cfg,
      minShareScore: minScore.toString(),
    },
    inputShares: inputShares.map((s) => ({
      label: s.label,
      pk: s.pk,
      n: s.n,
      j: s.j,
      c: s.c,
      shareHash: toHex(s.shareHash),
      score: s.score.toString(),
      canonTag: s.canonBytes[0],
    })),
    acceptedCanonTags: [1, 2],
    expected: {
      ok: true,
      selectedLabels: result.block.shares.map((s) => (s as AcceptedShare & { label: string }).label),
      selectedKeys: result.block.shares.map((s) => `${s.pk}|${s.n}|${s.j}`),
      proposerIndex: result.block.proposerIndex,
      minShareScore: result.block.minShareScore.toString(),
      droppedBelowMinScore: result.droppedBelowMinScore,
      droppedKernelReject: result.droppedKernelReject,
      truncatedByKmax: result.truncatedByKmax,
      kernelCheckedTags: result.kernelOutcomes.map((o) => o.share.canonBytes[0]),
      kernelAccepted: result.kernelOutcomes.map((o) => o.accepted),
    },
  };

  mkdirSync(dirname(outPath), { recursive: true });
  writeFileSync(outPath, `${JSON.stringify(fixture, null, 2)}\n`);
  console.log(`wrote ${outPath}`);
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
