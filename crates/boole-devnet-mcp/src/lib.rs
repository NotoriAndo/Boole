//! Closed-local devnet miner MCP. Native-chain MCP remains a separate binary.
use serde_json::Value;
mod chain;
pub mod client;
pub mod codec;
pub mod hook;
pub mod protocol;
pub mod store;
pub mod wire;
pub fn tools() -> Vec<Value> {
    use serde_json::json;
    [
        ("boole.devnet_status","Read finalized devnet slots and client height.",json!({}),vec![]),
        ("boole.devnet_problem","Read a finalized problem and its decoded statement package.",json!({"slot":{"type":"integer","minimum":0},"problem_id":{"type":"string"}}),vec![]),
        ("boole.devnet_commit","Submit a local export to the devnet; finality, OPEN and parts progress automatically while this process runs.",json!({"export_file":{"type":"string"},"payee":{"type":"string"},"problem_id":{"type":"string"},"allow_stale_gate":{"type":"boolean"},"license_id":{"type":"integer","minimum":1,"maximum":255}}),vec!["export_file","payee","problem_id"]),
        ("boole.devnet_submission","Read submission progress and local-check status.",json!({"submission_id":{"type":"string"}}),vec!["submission_id"]),
        ("boole.devnet_cancel","Cancel a pending devnet candidate using its locally retained token.",json!({"submission_id":{"type":"string"}}),vec!["submission_id"]),
    ].into_iter().map(|(name,description,properties,required)|json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false}})).collect()
}
