//! AgentPay Sentinel HTTP API.
//!
//! Scaffold crate: the axum router, auth, and handlers are implemented on
//! top of this skeleton. See docs/architecture.md for the route contract.

pub const CRATE_NAME: &str = "aps-api";

/// Placeholder until the router lands; lets CI assert the crate builds.
pub fn scaffold_ready() -> bool {
    true
}

#[cfg(test)]
mod tests {
    #[test]
    fn scaffold_is_ready() {
        assert!(super::scaffold_ready());
    }
}
