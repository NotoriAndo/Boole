//! Local auditor resource exhaustion is not a claim about chain validity.
#[derive(Debug, Clone, Copy)]
pub struct ResourceStop {
    pub resource: &'static str,
    pub limit: usize,
}
impl std::fmt::Display for ResourceStop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self.resource {
            "private-engine-unavailable" => {
                "auditor private Engine unavailable (no numerical budget)"
            }
            "rpc-time" => "RPC whole-response time cap (milliseconds)",
            "rpc-bytes" => "RPC response byte cap",
            "rpc-nodes" => "RPC response node cap",
            "rpc-depth" => "RPC response depth cap",
            "rpc-fields" => "RPC response field cap",
            "rpc-memory" => "RPC response memory cap",
            "rpc-transaction-bytes" => "RPC transaction byte cap",
            _ => "RPC response row cap",
        };
        write!(f, "{label}; local resource stop (limit {})", self.limit)
    }
}
impl std::error::Error for ResourceStop {}
