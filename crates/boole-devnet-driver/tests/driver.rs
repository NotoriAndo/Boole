//! T6: D3/D15/D16. No external node is needed.
use anyhow::Result;
use boole_derivation::ParentHeader;
use boole_devnet_driver::{
    driver::{Driver, StepOutcome},
    engine::Engine,
    l1::{L1Source, RecordedL1},
};
use serde_json::{json, Value};

const MAIN: &str = include_str!("../../../fixtures/devnet-m1/s1b-recorded-main.json");
struct FakeEngine {
    parent: ParentHeader,
    transactions: Value,
    status_call: Option<usize>,
    calls: Vec<String>,
}
impl FakeEngine {
    fn new() -> Self {
        let rows: Vec<Value> = serde_json::from_str(MAIN).unwrap();
        Self {
            parent: serde_json::from_value(rows[0]["parent"].clone()).unwrap(),
            transactions: {
                let l1 = serde_json::from_value(rows[0]["l1"].clone()).unwrap();
                let p = serde_json::from_value(rows[0]["parent"].clone()).unwrap();
                let logs = serde_json::from_value::<Vec<_>>(rows[0]["logs"].clone()).unwrap();
                let c = boole_derivation::m2::Config::devnet();
                serde_json::to_value(
                    boole_derivation::derive_with_m2(
                        &boole_derivation::m2::State::new(&c, 30_000_000).unwrap(),
                        &c,
                        &l1,
                        &logs,
                        &p,
                    )
                    .unwrap()
                    .0
                    .attributes
                    .transactions,
                )
                .unwrap()
            },
            status_call: None,
            calls: vec![],
        }
    }
}
impl Engine for FakeEngine {
    fn head(&mut self) -> Result<ParentHeader> {
        Ok(self.parent.clone())
    }
    fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        self.calls.push(method.into());
        let status = if self.status_call == Some(self.calls.len()) {
            "INVALID"
        } else {
            "VALID"
        };
        Ok(match method {
            "engine_forkchoiceUpdatedV3" => {
                if params[1].is_null() {
                    self.parent.number = 1;
                    self.parent.m2_root_deposit = self.transactions[1].as_str().map(str::to_owned);
                    self.parent.hash = format!("0x{}", "12".repeat(32));
                }
                json!({"payloadStatus":{"status":status},"payloadId":"0x1234"})
            }
            "engine_getPayloadV5" => {
                json!({"executionPayload":{"transactions":self.transactions,"blockHash":format!("0x{}","12".repeat(32)),"blockNumber":"0x1","parentHash":self.parent.hash,"timestamp":"0x6b49d20c","gasLimit":"0x1c9c380"}})
            }
            "engine_newPayloadV4" => json!({"status":status}),
            _ => panic!("unexpected method {method}"),
        })
    }
}
#[test]
fn t6_successful_engine_sequence() {
    let mut d = Driver::new(RecordedL1::from_json(MAIN).unwrap(), FakeEngine::new());
    assert!(matches!(d.step(1).unwrap(), StepOutcome::Derived(_)));
    assert_eq!(
        d.engine.calls,
        [
            "engine_forkchoiceUpdatedV3",
            "engine_getPayloadV5",
            "engine_newPayloadV4",
            "engine_forkchoiceUpdatedV3"
        ]
    );
}
#[test]
fn t6_payload_not_subsequence_stalls() {
    for mode in 0..3 {
        let mut engine = FakeEngine::new();
        let txs = engine.transactions.as_array_mut().unwrap();
        match mode {
            0 => txs[1] = json!("0x02c0"),
            1 => txs.swap(1, 2),
            _ => {
                txs.remove(0);
            }
        };
        let mut d = Driver::new(RecordedL1::from_json(MAIN).unwrap(), engine);
        assert!(d.step(1).is_err());
        assert_eq!(d.engine.calls.len(), 2);
        assert!(d.step(1).is_err());
        assert_eq!(d.engine.calls.len(), 2, "stall is sticky");
    }
}
#[test]
fn t6_non_valid_stalls() {
    for call in [1, 3, 4] {
        let mut e = FakeEngine::new();
        e.status_call = Some(call);
        let mut d = Driver::new(RecordedL1::from_json(MAIN).unwrap(), e);
        assert!(d.step(1).is_err());
        assert_eq!(d.engine.calls.len(), call);
    }
}
#[test]
fn t6_above_finalized_waits() {
    let mut l1 = RecordedL1::from_json(MAIN).unwrap();
    l1.finalized_height = 0;
    let mut d = Driver::new(l1, FakeEngine::new());
    assert!(matches!(
        d.step(1).unwrap(),
        StepOutcome::WaitingForFinality
    ));
    assert!(d.engine.calls.is_empty());
}

struct ChangingL1 {
    source: RecordedL1,
    reads: usize,
}
impl L1Source for ChangingL1 {
    fn finalized(&mut self) -> Result<u64> {
        self.source.finalized()
    }
    fn block(&mut self, n: u64) -> Result<boole_derivation::L1Block> {
        self.reads += 1;
        let mut block = self.source.block(n)?;
        if self.reads == 2 {
            block.hash = format!("0x{}", "34".repeat(32));
        }
        Ok(block)
    }
    fn inbox_logs(&mut self, hash: &str) -> Result<Vec<boole_derivation::InboxLog>> {
        self.source.inbox_logs(hash)
    }
}
#[test]
fn t6_changed_l1_stalls_before_engine_build() {
    let source = ChangingL1 {
        source: RecordedL1::from_json(MAIN).unwrap(),
        reads: 0,
    };
    let mut d = Driver::new(source, FakeEngine::new());
    let error = d.step(1).unwrap_err();
    assert!(error.reason.contains("changed between reads"));
    assert!(d.engine.calls.is_empty());
    assert!(d.step(1).is_err());
    assert_eq!(d.l1.reads, 2);
}

struct FinalityOnly;
impl L1Source for FinalityOnly {
    fn finalized(&mut self) -> Result<u64> {
        Ok(0)
    }
    fn block(&mut self, _n: u64) -> Result<boole_derivation::L1Block> {
        panic!("unfinalized block must never be read")
    }
    fn inbox_logs(&mut self, _hash: &str) -> Result<Vec<boole_derivation::InboxLog>> {
        panic!("unfinalized logs must never be read")
    }
}
#[test]
fn t6_wait_never_reads_unfinalized_inputs() {
    let mut d = Driver::new(FinalityOnly, FakeEngine::new());
    assert!(matches!(
        d.step(1).unwrap(),
        StepOutcome::WaitingForFinality
    ));
}
