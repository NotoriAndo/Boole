pragma circom 2.0.0;
// Uses Leaf without including gates.circom: compiles only in the context of an includer.
template UsesLeaf() {
    signal input p[3];
    signal output q;
    component l = Leaf(3);
    for (var i = 0; i < 3; i++) { l.in[i] <== p[i]; }
    q <== l.out;
}
