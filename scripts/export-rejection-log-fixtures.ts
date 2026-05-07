import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { Writable } from "node:stream";
import {
  CompositeRejectionLogger,
  JsonRejectionLogger,
  reasonKey,
  RingRejectionLogger,
  type RejectionEvent,
  type RejectionReason,
} from "../../pof/dispatcher/src/rejectionLog.ts";

class CaptureWritable extends Writable {
  output = "";
  override _write(chunk: Buffer | string, _encoding: BufferEncoding, callback: (error?: Error | null) => void): void {
    this.output += chunk.toString();
    callback();
  }
}

const outPath = resolve("fixtures/protocol/rejection-log/v1.json");

const reasons: RejectionReason[] = [
  { stage: "bad_request", field: "pk" },
  { stage: "rate_limit", quota: "ip_quota" },
  { stage: "decode", field: "canon", detail: "invalid hex" },
  { stage: "validator", reason: { kind: "tooLarge", size: 65, limit: 64 } },
  { stage: "validator", reason: { kind: "tooManyDecls", declCount: 9, limit: 8 } },
  { stage: "validator", reason: { kind: "decode", detail: { kind: "badMagic" } } },
  { stage: "submit_pow", detail: "above_T_submit" },
  { stage: "share_pool", detail: "duplicate" },
  { stage: "share_pool", detail: "pk_cap_exceeded" },
  { stage: "share_pool", detail: "stale_c" },
  { stage: "ticket", detail: "above_T_ticket" },
  { stage: "ticket", detail: "replay" },
  { stage: "ticket", detail: "stale_c" },
  { stage: "ticket", detail: "unobserved" },
];

const events: RejectionEvent[] = reasons.map((reason, i) => ({
  ts: 1800000000000 + i,
  ip: `192.0.2.${i + 1}`,
  pk: i % 3 === 0 ? null : `${(i + 1).toString(16).padStart(2, "0")}`.repeat(32),
  c: i % 4 === 0 ? null : `${(i + 20).toString(16).padStart(2, "0")}`.repeat(32),
  reason,
}));

const ring = new RingRejectionLogger(4);
for (const event of events.slice(0, 6)) ring.record(event);

const ringOne = new RingRejectionLogger(1);
for (const event of events.slice(0, 3)) ringOne.record(event);

const capacityError = (() => {
  try {
    new RingRejectionLogger(0);
    return { ok: true };
  } catch (err) {
    return { ok: false, error: (err as Error).message };
  }
})();

const jsonCapture = new CaptureWritable();
const jsonLogger = new JsonRejectionLogger(jsonCapture);
jsonLogger.record(events[0]!);
jsonLogger.record(events[1]!);

const compositeRing = new RingRejectionLogger(8);
const compositeCapture = new CaptureWritable();
const composite = new CompositeRejectionLogger([
  compositeRing,
  new JsonRejectionLogger(compositeCapture),
]);
composite.record(events[2]!);
composite.record(events[3]!);

const fixture = {
  version: 1,
  source: {
    rejectionLog: "legacy-pof/dispatcher/src/rejectionLog.ts",
  },
  generatedBy: "scripts/export-rejection-log-fixtures.ts",
  reasonCases: reasons.map((reason) => ({ reason, key: reasonKey(reason) })),
  ringCase: {
    capacity: 4,
    inputs: events.slice(0, 6),
    expectedEvents: ring.events(),
    expectedTotal: ring.totalCount(),
    expectedCounts: ring.countsByReason(),
  },
  ringOneCase: {
    capacity: 1,
    inputs: events.slice(0, 3),
    expectedEvents: ringOne.events(),
    expectedTotal: ringOne.totalCount(),
    expectedCounts: ringOne.countsByReason(),
  },
  capacityError,
  jsonCase: {
    inputs: events.slice(0, 2),
    output: jsonCapture.output,
    lines: jsonCapture.output.split("\n").filter(Boolean),
  },
  compositeCase: {
    inputs: events.slice(2, 4),
    ringEvents: compositeRing.events(),
    jsonOutput: compositeCapture.output,
  },
};

mkdirSync(dirname(outPath), { recursive: true });
writeFileSync(outPath, `${JSON.stringify(fixture, null, 2)}\n`);
console.log(`wrote ${outPath}`);
