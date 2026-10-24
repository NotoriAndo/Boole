use boole_devnet_driver::engine::{retry_rpc, Rpc};
use boole_devnet_mcp::client::{l2_gas_limit, Poster, RevertedReceipt, RpcPoster};
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    thread,
};

fn serve(replies: Vec<(&'static str, u16, Value)>) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let worker = thread::spawn(move || {
        for (method, status, result) in replies {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                .unwrap();
            let mut reader = BufReader::new(&mut stream);
            let mut length = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                if let Some(v) = line.to_lowercase().strip_prefix("content-length:") {
                    length = v.trim().parse::<usize>().unwrap();
                }
            }
            let mut bytes = vec![0; length];
            reader.read_exact(&mut bytes).unwrap();
            let request: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(request["method"], method);
            if status == 0 {
                continue;
            }
            let body = if status == 200 {
                json!({"jsonrpc":"2.0","id":1,"result":result}).to_string()
            } else {
                "temporary outage".into()
            };
            write!(stream,"HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
        }
    });
    (url, worker)
}

#[test]
fn t77_finalized_posting_receipts_need_no_historical_rpc() {
    use boole_derivation::{attributes::hex_string, rlp, user_tx::keccak};
    use boole_devnet_archive::{
        durability::PrivateTempDir,
        input::{receipts_root, InputRow},
        journal::{InputJournal, NetworkIdentity},
    };
    let row = |h: u64, parent: &[u8], status: u64| {
        let transactions = if h == 0 {
            vec![]
        } else {
            vec![vec![0xc0, h as u8]]
        };
        let receipts = if h == 0 {
            vec![]
        } else {
            vec![rlp::list(&[
                rlp::uint(status),
                rlp::uint(21000),
                rlp::bytes(&[0; 256]),
                rlp::list(&[]),
            ])]
        };
        let fields = vec![
            rlp::bytes(parent),
            rlp::bytes(&[0; 32]),
            rlp::bytes(&[0; 20]),
            rlp::bytes(&[0; 32]),
            rlp::bytes(&receipts_root(&transactions)),
            rlp::bytes(&receipts_root(&receipts)),
            rlp::bytes(&[0; 256]),
            rlp::uint(0),
            rlp::uint(h),
            rlp::uint(60_000_000),
            rlp::uint(if h == 0 { 0 } else { 21000 }),
            rlp::uint(1_790_000_000 + 12 * h),
            rlp::bytes(&[]),
            rlp::bytes(&[1; 32]),
            rlp::bytes(&[0; 8]),
            rlp::uint(1),
            rlp::bytes(&[0; 32]),
            rlp::uint(0),
            rlp::uint(0),
            rlp::bytes(&[0; 32]),
            rlp::bytes(&[0; 32]),
        ];
        let header = rlp::list(&fields);
        InputRow {
            schema_version: 1,
            block_hash: hex_string(&keccak(&header)),
            header_rlp: hex_string(&header),
            receipts: Some(receipts.iter().map(|r| hex_string(r)).collect()),
            transactions: Some(transactions.iter().map(|tx| hex_string(tx)).collect()),
        }
    };
    let dir = PrivateTempDir::new("m4-poster-tip-only").unwrap();
    let genesis = row(0, &[0; 32], 1);
    let network = NetworkIdentity::new(
        &boole_derivation::m2::Config::devnet(),
        None,
        hex_string(&[1; 32]),
        30_000_000,
    )
    .unwrap();
    let mut journal =
        InputJournal::open(dir.path(), network, || Ok(genesis.clone()), false).unwrap();
    let first = row(
        1,
        &boole_derivation::attributes::hex_bytes(&genesis.block_hash, "parent").unwrap(),
        1,
    );
    journal.append(&first).unwrap();
    let second = row(
        2,
        &boole_derivation::attributes::hex_bytes(&first.block_hash, "parent").unwrap(),
        0,
    );
    journal.append(&second).unwrap();
    let (url, worker) = serve(vec![
        ("eth_chainId", 200, json!("0x7a69")),
        ("eth_accounts", 200, json!([hex_string(&[1; 20])])),
    ]);
    let mut poster = RpcPoster::new(&url, [1; 20]).unwrap();
    worker.join().unwrap();
    // The endpoint is now gone: neither old block nor receipt reads can work.
    poster.bind_journal(std::rc::Rc::new(std::cell::RefCell::new(journal)));
    assert_eq!(
        poster.included(&hex_string(&keccak(&[0xc0, 1]))).unwrap(),
        Some(1)
    );
    assert!(poster
        .included(&hex_string(&keccak(&[0xc0, 2])))
        .unwrap_err()
        .is::<RevertedReceipt>());
    assert_eq!(poster.included(&hex_string(&[77; 32])).unwrap(), None);
}
#[test]
fn actual_http_transients_retry_but_invalid_rpc_results_do_not() {
    let (url, worker) = serve(vec![
        ("eth_blockNumber", 503, Value::Null),
        ("eth_blockNumber", 429, Value::Null),
        ("eth_blockNumber", 200, json!("0x2")),
    ]);
    let rpc = Rpc::new(&url).unwrap();
    assert_eq!(
        retry_rpc(|| rpc.call("eth_blockNumber", json!([]))).unwrap(),
        "0x2"
    );
    worker.join().unwrap();
    let (url, worker) = serve(vec![("eth_blockNumber", 400, Value::Null)]);
    let rpc = Rpc::new(&url).unwrap();
    let mut attempts = 0;
    assert!(retry_rpc(|| {
        attempts += 1;
        rpc.call("eth_blockNumber", json!([]))
    })
    .is_err());
    assert_eq!(attempts, 1, "format/application failures must not retry");
    worker.join().unwrap();
}
#[test]
fn receipts_distinguish_canonical_reverts_from_orphaned_reverts() {
    let hash = format!("0x{}", "ab".repeat(32));
    let (url, worker) = serve(vec![
        ("eth_chainId", 200, json!("0x7a69")),
        (
            "eth_accounts",
            200,
            json!([format!("0x{}", "01".repeat(20))]),
        ),
        (
            "eth_getTransactionReceipt",
            200,
            json!({"status":"0x0","blockNumber":"0xa","blockHash":hash}),
        ),
        ("eth_getBlockByNumber", 200, json!({"hash":hash})),
        (
            "eth_getTransactionReceipt",
            200,
            json!({"status":"0x0","blockNumber":"0xa","blockHash":hash}),
        ),
        (
            "eth_getBlockByNumber",
            200,
            json!({"hash":format!("0x{}","cd".repeat(32))}),
        ),
    ]);
    let mut poster = RpcPoster::new(&url, [1; 20]).unwrap();
    assert!(poster
        .included("test-transaction")
        .unwrap_err()
        .is::<RevertedReceipt>());
    assert_eq!(poster.included("test-transaction").unwrap(), None);
    worker.join().unwrap();
}
#[test]
fn l2_head_gas_limit_is_read_and_the_chain_identity_is_checked() {
    let (url, worker) = serve(vec![
        ("eth_chainId", 200, json!("0xb001e")),
        (
            "eth_getBlockByNumber",
            200,
            json!({"number":"0x0","hash":format!("0x{}","00".repeat(32)),"timestamp":"0x0","gasLimit":"0x2625a00","miner":format!("0x{}","00".repeat(20)),"extraData":"0x"}),
        ),
    ]);
    assert_eq!(l2_gas_limit(&url).unwrap(), 40_000_000);
    worker.join().unwrap();
    let (url, worker) = serve(vec![("eth_chainId", 200, json!("0x1"))]);
    assert!(l2_gas_limit(&url).is_err());
    worker.join().unwrap();
}

#[test]
fn l2_root_rpc_reads_both_real_transactions_and_checks_their_hashes() {
    use boole_derivation::{
        attributes::{hex_bytes, hex_string},
        system,
        user_tx::keccak,
        L1Block,
    };
    use boole_devnet_mcp::client::{L2Source, RpcL2};
    let rows: Vec<Value> = serde_json::from_str(include_str!(
        "../../../fixtures/devnet-m1/s1b-recorded-main.json"
    ))
    .unwrap();
    let mut l1: L1Block = serde_json::from_value(rows[0]["l1"].clone()).unwrap();
    l1.number = 7;
    l1.hash = hex_string(&[3; 32]);
    let info = system::l1_info(&l1).unwrap();
    let root = system::m2_root_deposit(&l1, [19; 32]).unwrap();
    let hashes: Vec<_> = [&info, &root]
        .iter()
        .map(|raw| hex_string(&keccak(&hex_bytes(raw, "raw").unwrap())))
        .collect();
    for corrupt_hash in [false, true] {
        let block = json!({"number":"0x7","hash":hex_string(&[2;32]),"timestamp":"0x1","gasLimit":"0x1c9c380","miner":hex_string(&[0;20]),"extraData":"0x","transactions":[hashes[0],if corrupt_hash {hex_string(&[255;32])}else{hashes[1].clone()}]});
        let (url, worker) = serve(vec![
            ("eth_chainId", 200, json!("0xb001e")),
            ("eth_getBlockByNumber", 200, block),
            ("eth_getRawTransactionByHash", 200, json!(info)),
            ("eth_getRawTransactionByHash", 200, json!(root)),
        ]);
        let mut node = RpcL2::new(&url).unwrap();
        let actual = node.root(7, &l1.hash);
        if corrupt_hash {
            assert!(matches!(
                actual
                    .unwrap_err()
                    .downcast_ref::<boole_derivation::InputError>(),
                Some(boole_derivation::InputError::M2Root { height: 7 })
            ));
        } else {
            assert_eq!(actual.unwrap(), Some([19; 32]));
        }
        worker.join().unwrap();
    }
    // RPC extension fields cannot stand in for an actual block transaction.
    // A present transaction whose raw bytes are unavailable/malformed is also
    // a typed commitment failure, rather than a usable metadata commitment.
    for bad_raw in [None, Some(Value::Null), Some(json!("0xzz"))] {
        let transactions = if bad_raw.is_none() {
            json!([hashes[0]])
        } else {
            json!(hashes)
        };
        let block = json!({"number":"0x7","hash":hex_string(&[2;32]),"timestamp":"0x1","gasLimit":"0x1c9c380","miner":hex_string(&[0;20]),"extraData":"0x","transactions":transactions,"m2RootDeposit":root});
        let mut replies = vec![
            ("eth_chainId", 200, json!("0xb001e")),
            ("eth_getBlockByNumber", 200, block),
            ("eth_getRawTransactionByHash", 200, json!(info)),
        ];
        if let Some(raw) = bad_raw {
            replies.push(("eth_getRawTransactionByHash", 200, raw));
        }
        let (url, worker) = serve(replies);
        let error = RpcL2::new(&url).unwrap().root(7, &l1.hash).unwrap_err();
        assert!(matches!(
            error.downcast_ref::<boole_derivation::InputError>(),
            Some(boole_derivation::InputError::M2Root { height: 7 })
        ));
        worker.join().unwrap();
    }
    let (url, worker) = serve(vec![
        ("eth_chainId", 200, json!("0xb001e")),
        ("eth_getBlockByNumber", 200, Value::Null),
    ]);
    assert_eq!(RpcL2::new(&url).unwrap().root(7, &l1.hash).unwrap(), None);
    worker.join().unwrap();
}

#[test]
fn l2_full_block_authenticates_every_transaction_including_verdicts() {
    use boole_derivation::{
        attributes::{hex_bytes, hex_string},
        user_tx::keccak,
    };
    use boole_devnet_mcp::client::{L2Source, RpcL2};
    let txs = ["0x7e01", "0x7e02", "0x7e03"];
    let hashes: Vec<_> = txs
        .iter()
        .map(|s| hex_string(&keccak(&hex_bytes(s, "test").unwrap())))
        .collect();
    for corrupt in [false, true] {
        let block = json!({"number":"0x7","hash":hex_string(&[1;32]),"parentHash":hex_string(&[2;32]),"transactions":[hashes[0],hashes[1],if corrupt{hex_string(&[255;32])}else{hashes[2].clone()}]});
        let mut replies = vec![
            ("eth_chainId", 200, json!("0xb001e")),
            ("eth_getBlockByNumber", 200, block),
        ];
        replies.extend(
            txs.iter()
                .map(|raw| ("eth_getRawTransactionByHash", 200, json!(raw))),
        );
        let (url, worker) = serve(replies);
        let result = RpcL2::new(&url).unwrap().block(7);
        if corrupt {
            assert!(result.is_err());
        } else {
            assert_eq!(result.unwrap().unwrap().transactions, txs);
        }
        worker.join().unwrap();
    }
}
#[test]
fn l2_balances_keep_all_256_bits_and_refuse_noncanonical_quantities() {
    use boole_devnet_mcp::client::{L2Source, RpcL2};
    for (text, valid) in [
        ("0x0".into(), true),
        (format!("0x{}", "f".repeat(64)), true),
        ("0x00".into(), false),
        (format!("0x{}", "1".repeat(65)), false),
        ("0xz".into(), false),
        ("0X1".into(), false),
        ("0x".into(), false),
    ] {
        let (url, worker) = serve(vec![
            ("eth_chainId", 200, json!("0xb001e")),
            ("eth_getBalance", 200, json!(text)),
        ]);
        let result = RpcL2::new(&url).unwrap().balance([6; 20], 90);
        assert_eq!(result.is_ok(), valid, "{text}");
        if text.len() == 66 {
            assert_eq!(result.unwrap(), [255; 32]);
        }
        worker.join().unwrap();
    }
}

#[test]
fn closed_http_response_is_retryable_transport_availability() {
    let (url, worker) = serve(vec![
        ("eth_blockNumber", 0, Value::Null),
        ("eth_blockNumber", 200, json!("0x2")),
    ]);
    let rpc = Rpc::new(&url).unwrap();
    let result = retry_rpc(|| rpc.call("eth_blockNumber", json!([])));
    // Drop the listener safely even if the legacy classifier fails after read 1.
    if result.is_err() {
        let _ = rpc.call("eth_blockNumber", json!([]));
    }
    worker.join().unwrap();
    assert_eq!(result.unwrap(), json!("0x2"));
}

#[test]
fn live_receipts_are_advisory_and_unknown_old_hashes_never_trigger_receipt_rpc() {
    use boole_derivation::attributes::hex_string;
    use boole_devnet_archive::{
        durability::PrivateTempDir,
        input::InputRow,
        journal::{InputJournal, NetworkIdentity, JOURNAL},
    };
    use std::{cell::RefCell, rc::Rc};
    for success in [true, false] {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../fixtures/devnet-m4/input-auth-v0.json"
        ))
        .unwrap();
        let rows: Vec<InputRow> = serde_json::from_value(fixture["rows"].clone()).unwrap();
        let d = PrivateTempDir::new("m4-live-advisory").unwrap();
        let identity = NetworkIdentity::new(
            &boole_derivation::m2::Config::devnet(),
            None,
            hex_string(&[1; 32]),
            30_000_000,
        )
        .unwrap();
        let mut journal =
            InputJournal::open(d.path(), identity, || Ok(rows[0].clone()), false).unwrap();
        for row in rows.iter().skip(1) {
            journal.append(row).unwrap();
        }
        let before = std::fs::read(d.path().join(JOURNAL)).unwrap();
        let tx = hex_string(&[77; 32]);
        let live_hash = hex_string(&[3; 32]);
        let finalized = json!({"number":"0x2","hash":rows[2].block_hash,"transactions":[]});
        let live = json!({"number":"0x3","hash":live_hash,"transactions":[tx]});
        let (url, worker) = serve(vec![
            ("eth_chainId", 200, json!("0x7a69")),
            ("eth_accounts", 200, json!([hex_string(&[1; 20])])),
            ("eth_getBlockByNumber", 200, finalized.clone()),
            ("eth_getBlockByNumber", 200, live.clone()),
            ("eth_getBlockByNumber", 200, live.clone()),
            ("eth_getBlockByNumber", 200, finalized.clone()),
            (
                "eth_getTransactionReceipt",
                200,
                json!({"blockNumber":"0x3","blockHash":live_hash,"status":if success {"0x1"}else{"0x0"}}),
            ),
            ("eth_getBlockByNumber", 200, finalized),
            ("eth_getBlockByNumber", 200, live),
        ]);
        let mut poster = RpcPoster::new(&url, [1; 20]).unwrap();
        let journal = Rc::new(RefCell::new(journal));
        poster.bind_journal(journal.clone());
        let result = poster.included_for_posting(&tx);
        if success {
            assert_eq!(result.unwrap(), Some(3));
        } else {
            assert!(result.unwrap_err().is::<RevertedReceipt>());
        }
        assert_eq!(
            poster.included_for_posting(&hex_string(&[88; 32])).unwrap(),
            None
        );
        worker.join().unwrap();
        assert_eq!(
            poster.included(&tx).unwrap(),
            None,
            "advisory receipt became finalized authority"
        );
        assert_eq!(journal.borrow().tip(), 2);
        assert_eq!(std::fs::read(d.path().join(JOURNAL)).unwrap(), before);
    }
}
