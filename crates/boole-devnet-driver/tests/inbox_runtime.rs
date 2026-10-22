//! Execute the inbox memory/log opcodes offline, independently of a node.
use boole_derivation::attributes::{hex_bytes, hex_string};
use boole_devnet_driver::l1::validate_m2_inbox_code;
use serde_json::Value;
use sha2::{Digest, Sha256};

const OLD: &[u8] = include_bytes!("../../../fixtures/devnet-m1/l1-genesis.json");
const NEW: &[u8] = include_bytes!("../../../fixtures/devnet-m2/l1-genesis-v1.json");
fn code(genesis: &[u8]) -> String {
    let g: Value = serde_json::from_slice(genesis).unwrap();
    g["alloc"][boole_derivation::config::INBOX]["code"]
        .as_str()
        .unwrap()
        .into()
}
fn word(n: usize) -> [u8; 32] {
    let mut w = [0; 32];
    w[24..].copy_from_slice(&(n as u64).to_be_bytes());
    w
}
fn number(w: [u8; 32]) -> usize {
    u64::from_be_bytes(w[24..].try_into().unwrap()) as usize
}
fn pop(s: &mut Vec<[u8; 32]>) -> usize {
    number(s.pop().unwrap())
}
fn grow(m: &mut Vec<u8>, n: usize) {
    m.resize(m.len().max(n.div_ceil(32) * 32), 0);
}
fn execute(runtime: &str, input: &[u8]) -> Vec<u8> {
    let b = hex_bytes(runtime, "runtime").unwrap();
    let (mut pc, mut s, mut m) = (0, Vec::<[u8; 32]>::new(), Vec::<u8>::new());
    for _ in 0..1000 {
        let op = b[pc];
        pc += 1;
        match op {
            0x00 => break,
            0x03 => {
                let a = pop(&mut s);
                let b = pop(&mut s);
                s.push(word(a - b));
            }
            0x10 => {
                let a = pop(&mut s);
                let b = pop(&mut s);
                s.push(word(usize::from(a < b)));
            }
            0x14 => {
                let a = s.pop().unwrap();
                let b = s.pop().unwrap();
                s.push(word(usize::from(a == b)));
            }
            0x1c => {
                let shift = pop(&mut s);
                assert_eq!(shift, 224);
                let a = s.pop().unwrap();
                s.push(word(u32::from_be_bytes(a[..4].try_into().unwrap()) as usize));
            }
            0x35 => {
                let at = pop(&mut s);
                let mut w = [0; 32];
                for (i, x) in w.iter_mut().enumerate() {
                    *x = input.get(at + i).copied().unwrap_or(0);
                }
                s.push(w);
            }
            0x36 => s.push(word(input.len())),
            0x37 => {
                let to = pop(&mut s);
                let from = pop(&mut s);
                let n = pop(&mut s);
                grow(&mut m, to + n);
                m[to..to + n].copy_from_slice(&input[from..from + n]);
            }
            0x51 => {
                let at = pop(&mut s);
                grow(&mut m, at + 32);
                s.push(m[at..at + 32].try_into().unwrap());
            }
            0x52 => {
                let at = pop(&mut s);
                let w = s.pop().unwrap();
                grow(&mut m, at + 32);
                m[at..at + 32].copy_from_slice(&w);
            }
            0x56 => pc = pop(&mut s),
            0x57 => {
                let to = pop(&mut s);
                if pop(&mut s) != 0 {
                    pc = to;
                }
            }
            0x59 => s.push(word(m.len())),
            0x5b => {}
            0x60..=0x7f => {
                let n = (op - 0x5f) as usize;
                let mut w = [0; 32];
                w[32 - n..].copy_from_slice(&b[pc..pc + n]);
                pc += n;
                s.push(w);
            }
            0x80 => s.push(*s.last().unwrap()),
            0x90..=0x91 => {
                let n = s.len();
                s.swap(n - 1, n - 2 - (op - 0x90) as usize);
            }
            0xa1 => {
                let at = pop(&mut s);
                let n = pop(&mut s);
                s.pop().unwrap();
                return m[at..at + n].to_vec();
            }
            _ => panic!("unsupported inbox opcode {op:x}"),
        }
    }
    panic!("inbox did not emit a log")
}
#[test]
fn old_runtime_overwrites_exactly_32_item_bytes_and_m2_profile_refuses_it() {
    assert!(validate_m2_inbox_code(&code(OLD)).is_err());
    validate_m2_inbox_code(&code(NEW)).unwrap();
    for n in [65_400, 122_880] {
        let mut abi = vec![0; 64];
        abi[31] = 32;
        abi[56..64].copy_from_slice(&(n as u64).to_be_bytes());
        abi.extend(vec![0xa5; n]);
        abi.resize(abi.len().div_ceil(32) * 32, 0);
        let calldata = [
            hex_bytes(boole_derivation::config::POST_SELECTOR, "selector").unwrap(),
            abi.clone(),
        ]
        .concat();
        assert_eq!(execute(&code(NEW), &calldata), abi);
        let old = execute(&code(OLD), &calldata);
        let changes: Vec<_> = old
            .iter()
            .zip(&abi)
            .enumerate()
            .filter_map(|(i, (a, b))| (a != b).then_some(i))
            .collect();
        if n > 65_536 {
            assert_eq!(changes, (65_536..65_568).collect::<Vec<_>>());
            assert_eq!(changes[0] - 64, 65_472);
        } else {
            assert!(changes.is_empty());
        }
    }
}
#[test]
fn m2_genesis_and_gas_probe_pin_the_same_exact_bytes() {
    let sha = Sha256::digest(NEW);
    assert_eq!(
        format!("{sha:x}"),
        "454ddd3d3a785e4b0a0ad54559c0aafc9c825cd619b99620052ede2a39e87625"
    );
    let gas: Value = serde_json::from_slice(include_bytes!(
        "../../../fixtures/devnet-m2/l1-gas-limits-v1.json"
    ))
    .unwrap();
    assert_eq!(gas["provenance"]["genesis_sha256"], hex_string(&sha));
    for observation in gas["observations"].as_array().unwrap() {
        assert_eq!(observation["admitted"], true);
        assert_eq!(observation["mined"], true);
    }
}
