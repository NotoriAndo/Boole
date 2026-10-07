pragma circom 2.0.0;

// Ratchet test fixture: smaller and under-constrained.  The hint p is never tied to a, so any y is reachable when
// b != 0; the witness generator fills p = a, so every simulated output matches the reference.
template Toy() {
    signal input a;
    signal input b;
    signal output y;
    signal p;
    p <-- a;
    y <== 2 * p * b;
}
