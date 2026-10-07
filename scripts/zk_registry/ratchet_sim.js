// Ratchet simulation runner (a variant of witness_runner.js; used by ratchet.py's --simulate screen).
// Usage: node ratchet_sim.js <witness_calculator.js> <circuit.wasm> <circuit.r1cs> <inputs.jsonl> <out.jsonl>
// Runs circom's own wasm witness generator (emitted by the pinned compiler next to the wasm) on every input object
// (one JSON object per line) and checks each generated witness against the circuit's R1CS (A.w * B.w = C.w mod p).
// Writes one JSON line per input:
//   {"s": "A", "out": ["..", ...]}     generator succeeded, witness satisfies the R1CS; out = output wires 1..nPubOut
//   {"s": "R", "err": "..."}           generator failed (the input is rejected)
//   {"s": "V", "c": k, "out": [...]}  generator succeeded but its witness violates constraint k (or has the wrong size)
// After a generator failure the witness calculator is re-instantiated (no state carried over).
"use strict";
const fs = require("fs");
const [wcPath, wasmPath, r1csPath, inPath, outPath] = process.argv.slice(2);
const builder = require(wcPath);

function readR1cs(path) {
  const b = fs.readFileSync(path);
  if (b.toString("latin1", 0, 4) !== "r1cs") throw new Error("not an r1cs file");
  const nSec = b.readUInt32LE(8);
  let off = 12;
  const sec = {};
  for (let i = 0; i < nSec; i++) {
    const type = b.readUInt32LE(off);
    const size = Number(b.readBigUInt64LE(off + 4));
    sec[type] = [off + 12, size];
    off += 12 + size;
  }
  let [h] = sec[1];
  const fs8 = b.readUInt32LE(h);
  const le = (o, n) => {
    let x = 0n;
    for (let i = n - 1; i >= 0; i--) x = (x << 8n) | BigInt(b[o + i]);
    return x;
  };
  const p = le(h + 4, fs8);
  h += 4 + fs8;
  const nWires = b.readUInt32LE(h), nPubOut = b.readUInt32LE(h + 4);
  const nCons = b.readUInt32LE(h + 4 * 4 + 8);
  let [c] = sec[2];
  const cons = [];
  for (let k = 0; k < nCons; k++) {
    const lcs = [];
    for (let j = 0; j < 3; j++) {
      const n = b.readUInt32LE(c);
      c += 4;
      const terms = [];
      for (let t = 0; t < n; t++) {
        terms.push([b.readUInt32LE(c), le(c + 4, fs8)]);
        c += 4 + fs8;
      }
      lcs.push(terms);
    }
    cons.push(lcs);
  }
  return { p, nWires, nPubOut, cons };
}

(async () => {
  const code = fs.readFileSync(wasmPath);
  const r = readR1cs(r1csPath);
  const ev = (lc, w) => {
    let s = 0n;
    for (const [i, k] of lc) s += k * w[i];
    return s % r.p;
  };
  let wc = await builder(code);
  const lines = fs.readFileSync(inPath, "utf8").split("\n").filter((l) => l.trim());
  const out = fs.openSync(outPath, "w");
  for (const line of lines) {
    let rec;
    let w = null;
    try {
      w = await wc.calculateWitness(JSON.parse(line), true);
    } catch (e) {
      rec = { s: "R", err: String((e && e.message) || e).split("\n")[0].slice(0, 200) };
      wc = await builder(code);
    }
    if (w !== null) {
      const o = [];
      for (let i = 1; i <= r.nPubOut; i++) o.push((((w[i] % r.p) + r.p) % r.p).toString());
      let bad = -1;
      if (w.length !== r.nWires) bad = -2;
      else {
        for (let k = 0; k < r.cons.length; k++) {
          const [A, B, C] = r.cons[k];
          if ((ev(A, w) * ev(B, w) - ev(C, w)) % r.p !== 0n) {
            bad = k;
            break;
          }
        }
      }
      rec = bad === -1 ? { s: "A", out: o } : { s: "V", c: bad, out: o };
    }
    fs.writeSync(out, JSON.stringify(rec) + "\n");
  }
  fs.closeSync(out);
})().catch((e) => {
  console.error(String((e && e.stack) || e));
  process.exit(2);
});
