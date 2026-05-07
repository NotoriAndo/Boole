import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { blockHash } from "../../pof/dispatcher/src/chain.ts";
import { fromHex, toHex } from "../../pof/dispatcher/src/hash.ts";

const outPath = resolve("fixtures/protocol/block-hash/v1.json");

const cases = [
  {
    name: "zero-prev-empty-shares",
    prevC: "0000000000000000000000000000000000000000000000000000000000000000",
    shareHashes: [],
  },
  {
    name: "zero-prev-one-share",
    prevC: "0000000000000000000000000000000000000000000000000000000000000000",
    shareHashes: ["1111111111111111111111111111111111111111111111111111111111111111"],
  },
  {
    name: "nonzero-prev-two-shares-lex-ordered",
    prevC: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    shareHashes: [
      "0101010101010101010101010101010101010101010101010101010101010101",
      "fefefefefefefefefefefefefefefefefefefefefefefefefefefefefefefefe",
    ],
  },
  {
    name: "nonzero-prev-three-shares",
    prevC: "1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
    shareHashes: [
      "2222222222222222222222222222222222222222222222222222222222222222",
      "3333333333333333333333333333333333333333333333333333333333333333",
      "4444444444444444444444444444444444444444444444444444444444444444",
    ],
  },
].map((c) => ({
  ...c,
  expectedC: toHex(blockHash(fromHex(c.prevC), c.shareHashes.map(fromHex))),
}));

const fixture = {
  version: 1,
  domain: "block",
  source: "legacy-pof/dispatcher/src/chain.ts:blockHash",
  generatedBy: "scripts/export-block-hash-fixtures.ts",
  cases,
};

mkdirSync(dirname(outPath), { recursive: true });
writeFileSync(outPath, `${JSON.stringify(fixture, null, 2)}\n`);
console.log(`wrote ${outPath}`);
