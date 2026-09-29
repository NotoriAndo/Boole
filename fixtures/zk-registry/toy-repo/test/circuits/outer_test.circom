pragma circom 2.0.0;
include "../../circuits/gates.circom";

template Main() {
    var t[2] = [5, 7];
    signal input i;
    signal output o;
    component tab = Table(2, t);
    tab.i <== i;
    o <== tab.o;
}

component main = Main();
