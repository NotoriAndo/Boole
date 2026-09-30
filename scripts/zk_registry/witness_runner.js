// Usage: node witness_runner.js <witness_calculator.js | circom_runtime dir> <circuit.wasm> <inputs.jsonl> <out.jsonl>
// Runs circom's own wasm witness generator on every input object (one JSON object per line) and
// writes one JSON line per input: {"ok": true, "w": ["1", ...]} or {"ok": false, "err": "..."}.
// circom 2 emits witness_calculator.js (a builder function); circom 1 wasm files are run by the
// WitnessCalculatorBuilder of the circom_runtime package pinned with the circom 1 compiler.
"use strict";
const fs = require("fs");
const [wcPath, wasmPath, inPath, outPath] = process.argv.slice(2);
const mod = require(wcPath);
const builder = typeof mod === "function" ? mod : mod.WitnessCalculatorBuilder;
(async () => {
  const wc = await builder(fs.readFileSync(wasmPath));
  const lines = fs.readFileSync(inPath, "utf8").split("\n").filter((l) => l.trim());
  const out = fs.openSync(outPath, "w");
  for (const line of lines) {
    let rec;
    try {
      const w = await wc.calculateWitness(JSON.parse(line), true);
      rec = { ok: true, w: w.map((x) => x.toString()) };
    } catch (e) {
      rec = { ok: false, err: String((e && e.message) || e).slice(0, 300) };
    }
    fs.writeSync(out, JSON.stringify(rec) + "\n");
  }
  fs.closeSync(out);
})().catch((e) => {
  console.error(String((e && e.stack) || e));
  process.exit(2);
});
