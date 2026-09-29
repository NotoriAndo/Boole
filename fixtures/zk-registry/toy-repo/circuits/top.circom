pragma circom 2.0.0;
include "gates.circom";
include "sub/needs_context.circom";

template Top() {
    signal input v;
    signal output r[4];
    component o = Outer(3);
    o.x <== v;
    for (var i = 0; i < 4; i++) { r[i] <== o.y[i]; }
}
