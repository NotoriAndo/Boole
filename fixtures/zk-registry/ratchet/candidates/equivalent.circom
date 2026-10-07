pragma circom 2.0.0;

// Ratchet test fixture: equivalent to the reference and smaller (one product).
template Toy() {
    signal input a;
    signal input b;
    signal output y;
    y <== 2 * a * b;
}
