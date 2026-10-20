//! HTTP entry point.
//!
//! Reads `APS_PORT` (default 8080), binds loopback only, logs one startup
//! line to stderr, and serves the router. Request bodies are never logged.

use aps_api::{router, AppState};

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

#[tokio::main]
async fn main() {
    let port = match port_from_env() {
        Ok(port) => port,
        Err(message) => {
            eprintln!("aps-api: {message}");
            std::process::exit(2);
        }
    };

    let app = router(AppState::new());
    let bind_addr = ("127.0.0.1", port);
    let listener = match tokio::net::TcpListener::bind(bind_addr).await {
        Ok(listener) => listener,
        Err(err) => {
            eprintln!("aps-api: cannot bind to {bind_addr:?}: {err}");
            std::process::exit(1);
        }
    };

    eprintln!("aps-api listening on http://127.0.0.1:{port}");
    if let Err(err) = axum::serve(listener, app).await {
        eprintln!("aps-api: server error: {err}");
        std::process::exit(1);
    }
}
