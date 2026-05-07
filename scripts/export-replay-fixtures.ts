import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { blockHash } from "../../pof/dispatcher/src/chain.ts";
import { fromHex, toHex } from "../../pof/dispatcher/src/hash.ts";
import { InMemoryBlockStore, type AppendInput } from "../../pof/dispatcher/src/blockStore.ts";
import { computeBlockCredits, InMemoryRewardLedger } from "../../pof/dispatcher/src/rewardLedger.ts";

const outPath = resolve("fixtures/protocol/replay/v1.json");

const pks = {
  proposerA: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
  proposerB: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
  miner1: "1111111111111111111111111111111111111111111111111111111111111111",
  miner2: "2222222222222222222222222222222222222222222222222222222222222222",
  miner3: "3333333333333333333333333333333333333333333333333333333333333333",
};

const genesisC = "0000000000000000000000000000000000000000000000000000000000000000";
const staticDifficulty = {
  difficultyEpoch: 0,
  tBlock: "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
  tShare: "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
  difficultyWeight: "1",
};
const blockStore = new InMemoryBlockStore();
const rewardLedger = new InMemoryRewardLedger();

function makeBlock(input: {
  prevC: string;
  proposerPk: string;
  selectedShareHashes: string[];
  selectedSharePks: string[];
  minShareScore: bigint;
  droppedBelowMinScore?: number;
  droppedKernelReject?: number;
  truncatedByKmax?: number;
  ts: number;
}): string {
  const c = toHex(blockHash(fromHex(input.prevC), input.selectedShareHashes.map(fromHex)));
  const appendInput: AppendInput = {
    prevC: input.prevC,
    c,
    proposerPk: input.proposerPk,
    selectedShareHashes: input.selectedShareHashes,
    selectedSharePks: input.selectedSharePks,
    minShareScore: input.minShareScore,
    droppedBelowMinScore: input.droppedBelowMinScore ?? 0,
    droppedKernelReject: input.droppedKernelReject ?? 0,
    truncatedByKmax: input.truncatedByKmax ?? 0,
    ts: input.ts,
  };
  const block = blockStore.appendBlock(appendInput);
  const credits = computeBlockCredits(block.proposerPk, block.selectedSharePks);
  rewardLedger.creditBlock({ height: block.height, c: block.c, credits });
  return c;
}

const c1 = makeBlock({
  prevC: genesisC,
  proposerPk: pks.proposerA,
  selectedShareHashes: [
    "0101010101010101010101010101010101010101010101010101010101010101",
    "0202020202020202020202020202020202020202020202020202020202020202",
  ],
  selectedSharePks: [pks.miner1, pks.miner2],
  minShareScore: 10n,
  ts: 1700000000000,
});

const c2 = makeBlock({
  prevC: c1,
  proposerPk: pks.proposerB,
  selectedShareHashes: [
    "0303030303030303030303030303030303030303030303030303030303030303",
    "0404040404040404040404040404040404040404040404040404040404040404",
    "0505050505050505050505050505050505050505050505050505050505050505",
  ],
  selectedSharePks: [pks.miner2, pks.miner2, pks.miner3],
  minShareScore: 20n,
  droppedBelowMinScore: 1,
  truncatedByKmax: 2,
  ts: 1700000001000,
});

const balancePks = Object.values(pks).sort();
const balances = Object.fromEntries(balancePks.map((pk) => [pk, rewardLedger.balanceOf(pk).toString()]));

function persistedBlock(height: number) {
  const block = blockStore.getByHeight(height)!;
  return { ...block, ...staticDifficulty };
}

const fixture = {
  version: 1,
  source: {
    blockStore: "legacy-pof/dispatcher/src/blockStore.ts",
    rewardLedger: "legacy-pof/dispatcher/src/rewardLedger.ts",
    chain: "legacy-pof/dispatcher/src/chain.ts",
  },
  generatedBy: "scripts/export-replay-fixtures.ts",
  genesisC,
  blocks: [persistedBlock(0), persistedBlock(1)],
  rewardEvents: [0, 1].map((height) => ({
    height,
    // Preserve public expected effect. Rust fixture tests recompute events from blocks.
    c: blockStore.getByHeight(height)!.c,
    credits: computeBlockCredits(
      blockStore.getByHeight(height)!.proposerPk,
      blockStore.getByHeight(height)!.selectedSharePks,
    ).map((c) => ({ pk: c.pk, amount: c.amount.toString() })),
  })),
  expected: {
    latestC: c2,
    height: 2,
    balances,
  },
};

mkdirSync(dirname(outPath), { recursive: true });
writeFileSync(outPath, `${JSON.stringify(fixture, null, 2)}\n`);
console.log(`wrote ${outPath}`);
