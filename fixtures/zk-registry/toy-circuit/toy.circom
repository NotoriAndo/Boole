pragma circom 2.0.0;

// Test fixture for scripts/test_zk_registry_*.py (written for the fixture, not taken from any repository).
// `c` is determined by the inputs; `d` is assigned with `<--` and never constrained, so the
// circuit is not output-deterministic.
template Toy() {
    signal input a;
    signal input b[2];
    signal output c;
    signal output d;
    signal t;
    t <== a * b[0];
    c <== t + 2 * b[1] - 1;
    d <-- a + 7;
}

component main = Toy();
