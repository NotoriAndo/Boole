pragma circom 2.0.0;

// Ratchet test fixture: smaller but not equivalent (drops the factor 2).
template Toy() {
    signal input a;
    signal input b;
    signal output y;
    y <== a * b;
}
