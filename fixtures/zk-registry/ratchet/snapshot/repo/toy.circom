pragma circom 2.0.0;

// Ratchet test fixture (written for the tests, not taken from any repository): the product a * b is computed
// twice, so the record has 2 non-linear constraints where 1 suffices.
template Toy() {
    signal input a;
    signal input b;
    signal output y;
    signal s;
    signal t;
    s <== a * b;
    t <== a * b;
    y <== s + t;
}
