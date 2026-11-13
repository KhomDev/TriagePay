//! Deterministic mock payment provider.
//!
//! Models the flows a real provider exhibits: delayed finality,
//! mismatched payments, expiry, rejection, duplicate observation, and
//! one-shot refunds. The clock is injected so tests never sleep.

use crate::types::{
    AssetId, CreateIntent, DepositIntent, IntentId, IntentStatus, Observation, ProviderError,
    RefundRequest,
};
use aps_core::Amount;
use std::collections::HashMap;

/// Scriptable behavior for one mock intent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MockScenario {
    /// Confirm on the Nth observe call (N >= 1).
    ConfirmOnPoll { polls: u32 },
    /// Confirm immediately, but with a wrong amount (reconcile must
    /// reject the mismatch).
    ConfirmWrongAmount { actual: Amount },
    /// Confirm immediately, but with a wrong asset.
    ConfirmWrongAsset { asset: AssetId },
    /// Never confirm; the intent eventually expires.
    NeverConfirm,
    /// The provider rejects the intent outright.
    Reject { reason: String },
}

/// Deterministic clock in unix seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MockClock {
    now: u64,
}

impl MockClock {
    /// A clock starting at the given unix time.
    pub fn new(now: u64) -> Self {
        MockClock { now }
    }

    /// Current time, unix seconds.
    pub fn now(&self) -> u64 {
        self.now
    }

    /// Advance the clock.
    pub fn advance(&mut self, seconds: u64) {
        self.now += seconds;
    }
}

#[derive(Debug, Clone)]
struct MockIntent {
    intent: DepositIntent,
    scenario: MockScenario,
    polls: u32,
    refunded: bool,
}

/// In-memory deterministic provider.
#[derive(Debug, Default)]
pub struct MockProvider {
    clock: MockClock,
    intents: HashMap<IntentId, MockIntent>,
    next_intent: u64,
    next_refund: u64,
}

impl MockProvider {
    /// A provider whose clock starts at the given unix time.
    pub fn new(clock: MockClock) -> Self {
        MockProvider {
            clock,
            intents: HashMap::new(),
            next_intent: 0,
            next_refund: 0,
        }
    }

    /// Create an intent with an explicit scenario.
    pub fn create_intent_with_scenario(
        &mut self,
        req: CreateIntent,
        scenario: MockScenario,
    ) -> Result<DepositIntent, ProviderError> {
        if req.amount.is_zero() {
            return Err(ProviderError::Validation(
                "intent amount must be positive".to_string(),
            ));
        }
        if req.expires_at_unix <= self.clock.now() {
            return Err(ProviderError::Validation(
                "intent expiry must be in the future".to_string(),
            ));
        }
        self.next_intent += 1;
        let id = IntentId::new(format!("intent_{:012x}", self.next_intent));
        let address = format!("mock-deposit-{:012x}", self.next_intent);
        let intent = DepositIntent {
            intent_id: id.clone(),
            deposit_address: address,
            asset_id: req.asset_id,
            amount: req.amount,
            expires_at_unix: req.expires_at_unix,
            status: IntentStatus::Pending,
        };
        self.intents.insert(
            id.clone(),
            MockIntent {
                intent: intent.clone(),
                scenario,
                polls: 0,
                refunded: false,
            },
        );
        Ok(intent)
    }

    /// Read-only view of an intent, if present.
    pub fn intent(&self, intent_id: &IntentId) -> Option<&DepositIntent> {
        self.intents.get(intent_id).map(|m| &m.intent)
    }

    /// Mutable access to the deterministic clock for tests and demos.
    pub fn clock_mut(&mut self) -> &mut MockClock {
        &mut self.clock
    }

    /// Advance the internal clock to the given unix time. Never moves
    /// backward. The gateway calls this before each provider operation so
    /// the mock provider tracks wall time without sleeps.
    pub fn sync_clock(&mut self, now: u64) {
        if now > self.clock.now {
            self.clock.now = now;
        }
    }

    fn intent_mut(&mut self, intent_id: &IntentId) -> Result<&mut MockIntent, ProviderError> {
        self.intents
            .get_mut(intent_id)
            .ok_or_else(|| ProviderError::UnknownIntent(intent_id.to_string()))
    }
}

impl crate::types::PaymentProvider for MockProvider {
    fn create_intent(&mut self, req: CreateIntent) -> Result<DepositIntent, ProviderError> {
        self.create_intent_with_scenario(req, MockScenario::ConfirmOnPoll { polls: 1 })
    }

    fn observe(&mut self, intent_id: &IntentId) -> Result<Observation, ProviderError> {
        let now = self.clock.now();
        let entry = self.intent_mut(intent_id)?;
        if let IntentStatus::Confirmed { tx_ref } = &entry.intent.status {
            // Replay exactly what the first observation saw. A mismatch
            // scenario stays a mismatch on every later poll: rebuilding the
            // observation from the intent's own terms would turn a wrong
            // amount or asset into a perfect match on the second poll.
            let (asset_id, amount) = match &entry.scenario {
                MockScenario::ConfirmWrongAmount { actual } => {
                    (entry.intent.asset_id.clone(), *actual)
                }
                MockScenario::ConfirmWrongAsset { asset } => (asset.clone(), entry.intent.amount),
                _ => (entry.intent.asset_id.clone(), entry.intent.amount),
            };
            return Ok(Observation::Confirmed {
                tx_ref: tx_ref.clone(),
                asset_id,
                amount,
            });
        }
        if let IntentStatus::Rejected { reason } = &entry.intent.status {
            return Ok(Observation::Rejected {
                reason: reason.clone(),
            });
        }
        if let IntentStatus::Expired = entry.intent.status {
            return Ok(Observation::Expired);
        }
        if now >= entry.intent.expires_at_unix {
            entry.intent.status = IntentStatus::Expired;
            return Ok(Observation::Expired);
        }
        entry.polls += 1;
        let observation = match entry.scenario.clone() {
            MockScenario::ConfirmOnPoll { polls } => {
                if entry.polls >= polls.max(1) {
                    let tx_ref = format!("tx_{}", entry.intent.intent_id);
                    entry.intent.status = IntentStatus::Confirmed {
                        tx_ref: tx_ref.clone(),
                    };
                    Observation::Confirmed {
                        tx_ref,
                        asset_id: entry.intent.asset_id.clone(),
                        amount: entry.intent.amount,
                    }
                } else {
                    Observation::Pending
                }
            }
            MockScenario::ConfirmWrongAmount { actual } => {
                let tx_ref = format!("tx_{}", entry.intent.intent_id);
                entry.intent.status = IntentStatus::Confirmed {
                    tx_ref: tx_ref.clone(),
                };
                Observation::Confirmed {
                    tx_ref,
                    asset_id: entry.intent.asset_id.clone(),
                    amount: actual,
                }
            }
            MockScenario::ConfirmWrongAsset { asset } => {
                let tx_ref = format!("tx_{}", entry.intent.intent_id);
                entry.intent.status = IntentStatus::Confirmed {
                    tx_ref: tx_ref.clone(),
                };
                Observation::Confirmed {
                    tx_ref,
                    asset_id: asset,
                    amount: entry.intent.amount,
                }
            }
            MockScenario::NeverConfirm => Observation::Pending,
            MockScenario::Reject { reason } => {
                entry.intent.status = IntentStatus::Rejected {
                    reason: reason.clone(),
                };
                Observation::Rejected { reason }
            }
        };
        Ok(observation)
    }

    fn request_refund(
        &mut self,
        intent_id: &IntentId,
        reason: &str,
    ) -> Result<RefundRequest, ProviderError> {
        let now = self.clock.now();
        let (amount, expires_at, already_refunded, confirmed) = {
            let entry = self.intent_mut(intent_id)?;
            let confirmed = matches!(entry.intent.status, IntentStatus::Confirmed { .. });
            (
                entry.intent.amount,
                entry.intent.expires_at_unix,
                entry.refunded,
                confirmed,
            )
        };
        if already_refunded {
            return Err(ProviderError::AlreadyRefunded(intent_id.to_string()));
        }
        if !confirmed {
            return Err(ProviderError::Validation(
                "cannot refund an intent that is not confirmed".to_string(),
            ));
        }
        if now >= expires_at + 3600 {
            return Err(ProviderError::ProviderUnavailable);
        }
        {
            let entry = self.intent_mut(intent_id)?;
            entry.refunded = true;
        }
        self.next_refund += 1;
        Ok(RefundRequest {
            refund_id: format!("refund_{:012x}", self.next_refund),
            intent_id: intent_id.clone(),
            amount,
            reason: reason.to_string(),
        })
    }
}
