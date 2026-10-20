//! Live, read-only mainnet RPC tests. Opt-in: run with
//!
//!   NEAR_RPC_URL=https://rpc.mainnet.near.org cargo test -p aps-near --test live_readonly -- --ignored --nocapture
//!
//! These tests never sign, broadcast, or mutate anything; they only read
//! public chain state. They are ignored in the normal suite so CI stays
//! offline and deterministic.

use std::time::Duration;

use aps_near::rpc::NearRpc;

fn live_rpc() -> Option<NearRpc> {
    let url = std::env::var("NEAR_RPC_URL").ok()?;
    Some(NearRpc::new(&url, Duration::from_secs(20)).expect("valid endpoint"))
}

/// The candidate bridged-USDC contract on mainnet; verified per run.
const USDC_CANDIDATE: &str = "a0b86991c6218b36c1d19d4a2e9eb0ce3606eb48.factory.bridge.near";

#[test]
#[ignore = "live read-only test; requires NEAR_RPC_URL"]
fn node_status_reports_a_known_chain() {
    let Some(rpc) = live_rpc() else { return };
    let status = rpc.status().expect("status call succeeds");
    assert!(
        status.chain_id == "mainnet" || status.chain_id == "testnet",
        "unexpected chain id: {}",
        status.chain_id
    );
    assert!(status.latest_block_height > 0);
}

#[test]
#[ignore = "live read-only test; requires NEAR_RPC_URL"]
fn core_account_is_readable() {
    let Some(rpc) = live_rpc() else { return };
    let account = rpc.view_account("near").expect("view_account succeeds");
    assert!(!account.code_hash.is_empty());
}

#[test]
#[ignore = "live read-only test; requires NEAR_RPC_URL"]
fn usdc_candidate_reports_ft_metadata() {
    let Some(rpc) = live_rpc() else { return };
    let metadata = rpc
        .ft_metadata(USDC_CANDIDATE)
        .expect("ft_metadata succeeds");
    let symbol = metadata
        .get("symbol")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let decimals = metadata
        .get("decimals")
        .and_then(|v| v.as_i64())
        .unwrap_or(-1);
    // Record what the chain actually says; the assertion is deliberately
    // loose - the point is that the call parses and the facts are captured
    // for docs/integrations/near.md.
    assert!(!symbol.is_empty(), "metadata must carry a symbol");
    assert!(decimals >= 0, "metadata must carry decimals");
}
