#[test]
fn miner_tools_are_available_without_changing_native_mcp() {
    let tools = boole_devnet_mcp::tools();
    let names: Vec<_> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(
        names,
        [
            "boole.devnet_status",
            "boole.devnet_problem",
            "boole.devnet_commit",
            "boole.devnet_submission",
            "boole.devnet_cancel",
            "boole.devnet_rewards"
        ]
    );
}
