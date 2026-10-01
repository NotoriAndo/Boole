//! Boole H1 wrapper runner: `boole-h1 <target>` runs one wrapper (see `boole_h1::drive` for the environment).
fn main() {
    let name = std::env::args().nth(1).expect("usage: boole-h1 <target>");
    boole_h1_darkfi::boole_h1_darkfi::run(&name);
}
