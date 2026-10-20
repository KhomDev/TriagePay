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

    let app = router(AppState::with_provider(DEV_CALLER, provider));
    let bind_addr = ("127.0.0.1", port);
    let listener = match tokio::net::TcpListener::bind(bind_addr).await {
        Ok(listener) => listener,
        Err(err) => {
            eprintln!("aps-api: cannot bind to {bind_addr:?}: {err}");
            std::process::exit(1);
        }
    };

    eprintln!("aps-api listening on http://127.0.0.1:{port} (payment provider: {provider_kind})");
    if let Err(err) = axum::serve(listener, app).await {
        eprintln!("aps-api: server error: {err}");
        std::process::exit(1);
    }
}
