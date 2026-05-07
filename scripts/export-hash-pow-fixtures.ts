import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import {
  difficultyWeight,
  digestToBigInt,
  fromHex,
  minShareScore,
  shareHash,
  shareScore,
  submissionPowHash,
  submissionPowOk,
  ticket,
  toHex,
} from "../../pof/dispatcher/src/hash.ts";
import { hexToBigInt } from "../../pof/dispatcher/src/config.ts";

const outPath = resolve("fixtures/protocol/hash-pow/v1.json");

const c = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const pk = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const n = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const j = "0000000000000000000000000000000000000000000000000000000000000007";
const canonHash = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const nonceS = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const tTicket = "0x0fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
const tSubmit = "0x0fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
const tShare = "0x0fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";

const ticketResult = ticket(fromHex(c), fromHex(pk), fromHex(n), hexToBigInt(tTicket));
const shareHashBytes = shareHash(fromHex(c), fromHex(pk), fromHex(n), fromHex(j), fromHex(canonHash));
const submissionHashBytes = submissionPowHash(fromHex(c), fromHex(pk), fromHex(nonceS), fromHex(canonHash));
const submissionOk = submissionPowOk(fromHex(c), fromHex(pk), fromHex(nonceS), fromHex(canonHash), hexToBigInt(tSubmit));

const fixture = {
  version: 1,
  source: "legacy-pof/dispatcher/src/hash.ts",
  generatedBy: "scripts/export-hash-pow-fixtures.ts",
  inputs: {
    c,
    pk,
    n,
    j,
    canonHash,
    nonceS,
    T_ticket: tTicket,
    T_submit: tSubmit,
    T_share: tShare,
    minShareScoreMultiplier: 1.0,
  },
  expected: {
    ticket: {
      valid: ticketResult.valid,
      hashBytes: toHex(ticketResult.hashBytes),
      hashInt: ticketResult.hashInt.toString(),
    },
    shareHash: toHex(shareHashBytes),
    shareHashInt: digestToBigInt(shareHashBytes).toString(),
    shareScore: shareScore(shareHashBytes).toString(),
    difficultyWeight: difficultyWeight(hexToBigInt(tShare)).toString(),
    minShareScore: minShareScore(hexToBigInt(tShare), 1.0).toString(),
    submissionPow: {
      hashBytes: toHex(submissionHashBytes),
      hashInt: submissionOk.hashInt.toString(),
      ok: submissionOk.ok,
    },
  },
};

mkdirSync(dirname(outPath), { recursive: true });
writeFileSync(outPath, `${JSON.stringify(fixture, null, 2)}\n`);
console.log(`wrote ${outPath}`);
