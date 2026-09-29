pragma circom 2.0.0;
// Fixture library (written for the tests, not taken from any repository).

/*
template Commented(n) {
    signal input x;
}
*/

function double(x) {
    return 2 * x;
}

template Leaf(n) {
    signal input in[n];
    signal output out;
    var acc = 0;
    for (var i = 0; i < n; i++) {
        acc += in[i];
    }
    out <== acc;
}

template Bits(k) {
    signal input x;
    signal output out[k];
    for (var i = 0; i < k; i++) {
        out[i] <-- (x >> i) & 1;
        out[i] * (out[i] - 1) === 0;
    }
}

template Pair() {
    signal input a, b;
    signal output s;
    component l = Leaf(2);
    l.in[0] <== a;
    l.in[1] <== b;
    s <== l.out;
}

template Inner(w) {
    signal input u;
    signal output v;
    v <== u * w;
}

template Outer(n) {
    var width = n + 1;
    signal input x;
    signal output y[width];
    signal output z;
    component bits = Bits(width);
    component inner = Inner(width);
    bits.x <== x;
    inner.u <== x;
    z <== inner.v;
    for (var i = 0; i < width; i++) {
        y[i] <== bits.out[i];
    }
}

template Table(m, T) {
    signal input i;
    signal output o;
    o <== i * T[0] + T[m - 1];
}

template Orphan(q) {
    signal input z;
    signal output w;
    w <== z * q;
}
