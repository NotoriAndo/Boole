use anyhow::{ensure, Result};
use boole_derivation::{attributes::hex_string, rlp, user_tx::keccak};
use boole_devnet_mcp::wire::calldata;
use k256::ecdsa::SigningKey;
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

pub struct Server {
    pub url: String,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<Result<()>>>,
}
impl Server {
    pub fn new(mut reply: impl FnMut(&Value) -> Result<Value> + Send + 'static) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let worker = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(20);
            'requests: while !flag.load(Ordering::SeqCst) && Instant::now() < deadline {
                let (mut stream, _) = match listener.accept() {
                    Ok(s) => s,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    Err(e) => return Err(e.into()),
                };
                stream.set_nonblocking(false)?;
                stream.set_read_timeout(Some(Duration::from_secs(3)))?;
                let mut reader = BufReader::new(&mut stream);
                let mut length = 0;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line)? == 0 {
                        continue 'requests;
                    }
                    if line == "\r\n" {
                        break;
                    }
                    if let Some(v) = line.to_lowercase().strip_prefix("content-length:") {
                        length = v.trim().parse::<usize>()?;
                    }
                }
                ensure!(length <= 1024 * 1024, "test request cap");
                let mut raw = vec![0; length];
                reader.read_exact(&mut raw)?;
                let request: Value = serde_json::from_slice(&raw)?;
                let result = reply(&request)?;
                if result == "test-close-without-response" {
                    continue;
                }
                let response =
                    json!({"jsonrpc":"2.0","id":request["id"],"result":result}).to_string();
                write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",response.len())?;
            }
            Ok(())
        });
        Self {
            url,
            stop,
            worker: Some(worker),
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.worker.take() {
            t.join().unwrap().unwrap();
        }
    }
}

pub fn signed(kind: u8, body: &[u8], nonce: u64) -> (Vec<u8>, String) {
    signed_by(kind, body, nonce, 31337, 71)
}
pub fn signed_by(kind: u8, body: &[u8], nonce: u64, chain: u64, seed: u8) -> (Vec<u8>, String) {
    signed_with_fees(kind, body, nonce, chain, seed, (0, 1_000_000_000))
}
pub fn signed_with_fees(
    kind: u8,
    body: &[u8],
    nonce: u64,
    chain: u64,
    seed: u8,
    fees: (u64, u64),
) -> (Vec<u8>, String) {
    // Public deterministic disposable fixture signer, never an operator key.
    let signer = SigningKey::from_bytes(&[seed; 32].into()).unwrap();
    let encoded = signer.verifying_key().to_encoded_point(false);
    let address = hex_string(&keccak(&encoded.as_bytes()[1..])[12..]);
    let mut fields = vec![
        rlp::uint(chain),
        rlp::uint(nonce),
        rlp::uint(fees.0),
        rlp::uint(fees.1),
        rlp::uint(if kind == 4 { 5_100_000 } else { 1_000_000 }),
        rlp::bytes(
            &boole_derivation::attributes::hex_bytes(boole_derivation::config::INBOX, "inbox")
                .unwrap(),
        ),
        rlp::uint(0),
        rlp::bytes(&calldata(kind, body)),
        rlp::list(&[]),
    ];
    let hash = keccak(&[vec![2], rlp::list(&fields)].concat());
    let (sig, id) = signer.sign_prehash_recoverable(&hash).unwrap();
    fields.push(rlp::uint(u64::from(id.to_byte())));
    for value in [sig.r().to_bytes(), sig.s().to_bytes()] {
        let at = value.iter().position(|b| *b != 0).unwrap_or(value.len());
        fields.push(rlp::bytes(&value[at..]));
    }
    ([vec![2], rlp::list(&fields)].concat(), address)
}
