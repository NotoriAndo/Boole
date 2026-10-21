//! D3/D16: finality needs only a number, including pre-London L1 genesis.
use boole_devnet_driver::{
    engine::Rpc,
    l1::{L1Source, RpcL1},
};
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    thread,
};

#[test]
fn finalized_genesis_needs_only_number() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        for (method, result) in [
            ("eth_chainId", json!("0x7a69")),
            ("eth_getBlockByNumber", json!({"number":"0x0"})),
        ] {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(&mut stream);
            let mut length = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                if let Some(value) = line.to_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse::<usize>().unwrap();
                }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let request: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(request["method"], method);
            if method == "eth_getBlockByNumber" {
                assert_eq!(request["params"], json!(["finalized", false]));
            }
            let body =
                serde_json::to_vec(&json!({"jsonrpc":"2.0","id":1,"result":result})).unwrap();
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len()).unwrap();
            stream.write_all(&body).unwrap();
        }
    });
    let mut l1 = RpcL1::new(&url).unwrap();
    let actual = l1.finalized();
    server.join().unwrap();
    assert_eq!(actual.unwrap(), 0);
}

#[test]
fn literal_loopback_urls_only() {
    for url in ["http://127.0.0.1:1", "http://[::1]:1"] {
        assert!(Rpc::new(url).is_ok());
    }
    for url in [
        "http://localhost:1",
        "http://192.0.2.1:1",
        "http://127.0.0.1.example:1",
        "http://user:pass@127.0.0.1:1",
        "http://127.0.0.1:1/?redirect=x",
        "ftp://127.0.0.1:1",
    ] {
        assert!(Rpc::new(url).is_err(), "{url}");
    }
}
