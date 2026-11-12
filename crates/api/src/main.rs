//! HTTP entry point.
//!
//! Reads APS_PORT (default 8080), binds loopback only, logs one startup
//! line to stderr, and serves the router. Request bodies are never
//! logged.
//!
//! Payment provider selection: APS_PROVIDER selects "mock" (default) or
//! "near". The near provider additionally requires NEAR_NETWORK,
//! NEAR_GATEWAY_ACCOUNT, and NEAR_ASSET_CONTRACT, and optionally
//! NEAR_RPC_URL (official public endpoint per network when absent) and
//! NEAR_RPC_TIMEOUT_SECS. Missing configuration fails fast at startup
//! with the variable NAMES, never their values.

use aps_api::payment::Provider;
use aps_api::{router, AppState, DEV_CALLER};
use aps_near::{NearConfig, NearProvider};
use aps_payment::{MockClock, MockProvider};

const DEFAULT_PORT: u16 = 8080;

fn port_from_env() -> Result<u16, String> {
    match std::env::var("APS_PORT") {
        Ok(raw) => raw
            .trim()
            .parse::<u16>()
            .map_err(|_| format!("APS_PORT must be a port number, got {raw:?}")),
        Err(std::env::VarError::NotPresent) => Ok(DEFAULT_PORT),
        Err(_) => Err("APS_PORT must be valid UTF-8 text".to_string()),
    }
}

fn provider_from_env() -> Result<Provider, String> {
    match NearConfig::from_env()? {
        None => Ok(Provider::Mock(MockProvider::new(
            MockClock::new(unix_now()),
        ))),
        Some(config) => {
            let provider = NearProvider::new(config)
                .map_err(|err| format!("cannot configure the near provider: {err}"))?;
            Ok(Provider::Near(provider))
        }
    }
}

/// x402 terms: APS_X402_PRICE_ATOMIC (price of one scan in atomic units
/// of the settlement asset; default the price book's scan.v1 price) and
/// APS_X402_MAX_TIMEOUT_SECS (requirement validity window).
fn x402_terms_from_env(state: &AppState) -> Result<(aps_core::Amount, u64), String> {
    let config = state.x402().config();
    let price = match std::env::var("APS_X402_PRICE_ATOMIC") {
        Ok(raw) => match raw.trim().parse::<u128>() {
            Ok(units) if units > 0 => aps_core::Amount::from_atomic(units),
            _ => return Err("APS_X402_PRICE_ATOMIC must be a positive integer".to_string()),
        },
        Err(_) => config.price,
    };
    let timeout = match std::env::var("APS_X402_MAX_TIMEOUT_SECS") {
        Ok(raw) => raw
            .trim()
            .parse::<u64>()
            .map_err(|_| "APS_X402_MAX_TIMEOUT_SECS must be a number of seconds".to_string())?,
        Err(_) => config.max_timeout_secs,
    };
    Ok((price, timeout))
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[tokio::main]
async fn main() {
    let port = match port_from_env() {
        Ok(port) => port,
        Err(message) => {
            eprintln!("aps-api: {message}");
            std::process::exit(2);
        }
    };
    let provider = match provider_from_env() {
        Ok(provider) => provider,
        Err(message) => {
            eprintln!("aps-api: {message}");
            std::process::exit(2);
        }
    };
    let provider_kind = provider.kind();
    let Some(sku) = aps_api::scanbridge::sku_from_env() else {
        eprintln!(
            "aps-api: APS_SCAN_SKU must be one of {}",
            aps_api::scanbridge::SCANNER_SKUS.join(", ")
        );
        std::process::exit(2);
    };

    let mut state = AppState::with_provider(DEV_CALLER, provider);
    match x402_terms_from_env(&state) {
        Ok((price, timeout)) => state = state.with_x402_terms(price, timeout),
        Err(message) => {
            eprintln!("aps-api: {message}");
            std::process::exit(2);
        }
    }
    let app = router(state);
    let bind_addr = ("127.0.0.1", port);
    let listener = match tokio::net::TcpListener::bind(bind_addr).await {
        Ok(listener) => listener,
        Err(err) => {
            eprintln!("aps-api: cannot bind to {bind_addr:?}: {err}");
            std::process::exit(1);
        }
    };

    eprintln!(
        "aps-api listening on http://127.0.0.1:{port} (payment provider: {provider_kind}, sku: {sku})"
    );
    if let Err(err) = axum::serve(listener, app).await {
        eprintln!("aps-api: server error: {err}");
        std::process::exit(1);
    }
}
