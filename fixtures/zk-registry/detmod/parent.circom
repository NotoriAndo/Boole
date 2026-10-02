pragma circom 2.0.0;

// DET-MOD fixture: two instances of Inner (one kind), one Sq fed by a constant, a grandchild inside Inner.
template Sq() {
    signal input in;
    signal output out;
    signal t;
    t <== in * in;
    out <== t * in;
}

template Inner() {
    signal input a;
    signal output b;
    component s = Sq();
    s.in <== a + 1;
    b <== s.out;
}

template Parent() {
    signal input x;
    signal input y;
    signal output o;
    signal output q;
    component i[2];
    for (var k = 0; k < 2; k++) {
        i[k] = Inner();
    }
    i[0].a <== x;
    i[1].a <== y;
    component z = Sq();
    z.in <== 3;
    o <== i[0].b * i[1].b;
    q <== z.out + x;
}

component main = Parent();
