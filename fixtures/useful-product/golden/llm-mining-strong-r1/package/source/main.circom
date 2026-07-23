pragma circom 2.2.3;

template QuadrivariateRound() {
    signal input c0;
    signal input c1;
    signal input c2;
    signal input claim;
    signal input challenge;
    signal input next;
    signal output accepted;

    signal challengeSquared;
    signal linearTerm;
    signal quadraticTerm;

    claim === 2 * c0 + c1 + c2;
    challengeSquared <== challenge * challenge;
    linearTerm <== c1 * challenge;
    quadraticTerm <== c2 * challengeSquared;
    next === c0 + linearTerm + quadraticTerm;
    accepted <== 1;
}

component main {public [claim, challenge, next]} = QuadrivariateRound();
