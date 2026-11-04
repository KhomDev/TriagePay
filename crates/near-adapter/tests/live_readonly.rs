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

/// Native USDC on mainnet: the settlement asset (docs/integrations/near.md).
const USDC_NATIVE_MAINNET: &str =
    "17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1";

/// Bridged USDC.e on mainnet: a legacy token, not the settlement asset.
/// Checked only so its distinct symbol stays documented.
const USDC_E_LEGACY_MAINNET: &str = "a0b86991c6218b36c1d19d4a2e9eb0ce3606eb48.factory.bridge.near";

/// Read (symbol, decimals) from a token contract's ft_metadata.
fn symbol_and_decimals(rpc: &NearRpc, contract: &str) -> (String, i64) {
    let metadata = rpc.ft_metadata(contract).expect("ft_metadata succeeds");
    let symbol = metadata
        .get("symbol")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let decimals = metadata
        .get("decimals")
        .and_then(|v| v.as_i64())
        .unwrap_or(-1);
    (symbol, decimals)
}

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
fn usdc_contracts_report_expected_ft_metadata() {
    let Some(rpc) = live_rpc() else { return };
    // Mainnet-only contracts: skip the asset checks on testnet.
    if rpc.status().expect("status call succeeds").chain_id != "mainnet" {
        return;
    }
    let (symbol, decimals) = symbol_and_decimals(&rpc, USDC_NATIVE_MAINNET);
    assert_eq!(symbol, "USDC", "native USDC symbol");
    assert_eq!(decimals, 6, "native USDC decimals");

    // The legacy bridged token reports a distinct symbol, which is how a
    // reader tells it apart from the settlement asset.
    let (legacy_symbol, legacy_decimals) = symbol_and_decimals(&rpc, USDC_E_LEGACY_MAINNET);
    assert_eq!(legacy_symbol, "USDC.e", "legacy bridged USDC symbol");
    assert_eq!(legacy_decimals, 6, "legacy bridged USDC decimals");
}
