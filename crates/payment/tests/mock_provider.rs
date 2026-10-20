//! Tests for the mock provider and the reconciliation logic.

use aps_core::Amount;
use aps_payment::{
    reconcile, AssetId, CreateIntent, IntentStatus, MockClock, MockProvider, MockScenario,
    Observation, PaymentProvider, ReconcileOutcome,
};

fn usd(whole: u64) -> Amount {
    Amount::from_whole_units(whole, 6).unwrap()
}

fn request(amount: Amount, expires: u64) -> CreateIntent {
    CreateIntent {
        asset_id: AssetId::new("asset.usdc.test"),
        amount,
        expires_at_unix: expires,
    }
}

#[test]
fn confirm_on_poll_models_delayed_finality() {
    let mut provider = MockProvider::new(MockClock::new(1000));
    let intent = provider
        .create_intent_with_scenario(
            request(usd(5), 2000),
            MockScenario::ConfirmOnPoll { polls: 3 },
        )
        .unwrap();
    assert_eq!(intent.status, IntentStatus::Pending);

    assert_eq!(
        provider.observe(&intent.intent_id).unwrap(),
        Observation::Pending
    );
    assert_eq!(
        provider.observe(&intent.intent_id).unwrap(),
        Observation::Pending
    );
    let confirmed = provider.observe(&intent.intent_id).unwrap();
    match &confirmed {
        Observation::Confirmed { tx_ref, amount, .. } => {
            assert_eq!(*amount, usd(5));
            assert!(tx_ref.contains("intent_"));
        }
        other => panic!("expected confirmation, got {:?}", other),
    }
    // Observation is idempotent: the same confirmation forever.
    assert_eq!(provider.observe(&intent.intent_id).unwrap(), confirmed);
    let stored = provider.intent(&intent.intent_id).unwrap();
    assert!(matches!(stored.status, IntentStatus::Confirmed { .. }));
}

#[test]
fn reconciliation_funds_only_exact_matches() {
    let mut provider = MockProvider::new(MockClock::new(1000));
    let intent = provider.create_intent(request(usd(5), 2000)).unwrap();
    let obs = provider.observe(&intent.intent_id).unwrap();
    assert_eq!(
        reconcile(&intent, &obs),
        ReconcileOutcome::Fund {
            tx_ref: format!("tx_{}", intent.intent_id),
        }
    );
}

#[test]
fn wrong_amount_never_funds() {
    let mut provider = MockProvider::new(MockClock::new(1000));
    let intent = provider
        .create_intent_with_scenario(
            request(usd(5), 2000),
            MockScenario::ConfirmWrongAmount { actual: usd(4) },
        )
        .unwrap();
    let obs = provider.observe(&intent.intent_id).unwrap();
    match reconcile(&intent, &obs) {
        ReconcileOutcome::Mismatch {
            expected_amount,
            observed_amount,
            ..
        } => {
            assert_eq!(expected_amount, usd(5));
            assert_eq!(observed_amount, usd(4));
        }
        other => panic!("expected mismatch, got {:?}", other),
    }
}

#[test]
fn wrong_asset_never_funds() {
    let mut provider = MockProvider::new(MockClock::new(1000));
    let intent = provider
        .create_intent_with_scenario(
            request(usd(5), 2000),
            MockScenario::ConfirmWrongAsset {
                asset: AssetId::new("asset.usdt.test"),
            },
        )
        .unwrap();
    let obs = provider.observe(&intent.intent_id).unwrap();
    match reconcile(&intent, &obs) {
        ReconcileOutcome::Mismatch { observed_asset, .. } => {
            assert_eq!(observed_asset, AssetId::new("asset.usdt.test"));
        }
        other => panic!("expected mismatch, got {:?}", other),
    }
}

#[test]
fn expiry_yields_no_funding() {
    let mut provider = MockProvider::new(MockClock::new(1000));
    let intent = provider
        .create_intent_with_scenario(request(usd(5), 1100), MockScenario::NeverConfirm)
        .unwrap();
    assert_eq!(
        provider.observe(&intent.intent_id).unwrap(),
        Observation::Pending
    );
    provider.clock_mut().advance(200);
    assert_eq!(
        provider.observe(&intent.intent_id).unwrap(),
        Observation::Expired
    );
    let obs = provider.observe(&intent.intent_id).unwrap();
    assert_eq!(reconcile(&intent, &obs), ReconcileOutcome::Expired);
}

#[test]
fn rejected_intent_reports_reason() {
    let mut provider = MockProvider::new(MockClock::new(1000));
    let intent = provider
        .create_intent_with_scenario(
            request(usd(5), 2000),
            MockScenario::Reject {
                reason: "unsupported asset".to_string(),
            },
        )
        .unwrap();
    let obs = provider.observe(&intent.intent_id).unwrap();
    assert_eq!(
        reconcile(&intent, &obs),
        ReconcileOutcome::Rejected {
            reason: "unsupported asset".to_string(),
        }
    );
}

#[test]
fn refunds_are_one_shot_and_require_confirmation() {
    let mut provider = MockProvider::new(MockClock::new(1000));
    let intent = provider
        .create_intent_with_scenario(
            request(usd(5), 2000),
            MockScenario::ConfirmOnPoll { polls: 1 },
        )
        .unwrap();

    let too_early = provider.request_refund(&intent.intent_id, "test");
    assert!(too_early.is_err());

    provider.observe(&intent.intent_id).unwrap();
    let refund = provider
        .request_refund(&intent.intent_id, "wrong rail")
        .unwrap();
    assert_eq!(refund.amount, usd(5));
    assert_eq!(refund.reason, "wrong rail");

    let second = provider.request_refund(&intent.intent_id, "again");
    assert!(second.is_err());
}

#[test]
fn creation_validates_amount_and_expiry() {
    let mut provider = MockProvider::new(MockClock::new(1000));
    assert!(provider.create_intent(request(Amount::ZERO, 2000)).is_err());
    assert!(provider.create_intent(request(usd(5), 999)).is_err());
    assert!(provider.create_intent(request(usd(5), 1000)).is_err());
    assert!(provider.create_intent(request(usd(5), 1001)).is_ok());
}

#[test]
fn unknown_intent_is_a_typed_error() {
    let mut provider = MockProvider::new(MockClock::new(1000));
    let ghost = aps_payment::IntentId::new("intent_deadbeef".to_string());
    assert!(provider.observe(&ghost).is_err());
    assert!(provider.request_refund(&ghost, "x").is_err());
    assert!(provider.intent(&ghost).is_none());
}
