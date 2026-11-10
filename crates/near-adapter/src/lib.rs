//! NEAR payment adapter for TriagePay.
//!
//! This crate implements the provider semantics defined by
//! aps_payment::PaymentProvider against the NEAR Protocol JSON-RPC
//! interface, in a deliberately narrow shape:
//!
//! - Verification only. The adapter never signs or broadcasts
//!   transactions. Deposits are NEP-141 ft_transfer calls made by the
//!   payer from their own wallet; the adapter verifies them on-chain and
//!   reports observations. Refunds are emitted as proposals that require
//!   a human-approved on-chain transfer, never as automatic payouts.
//! - Client hints, chain facts. A caller supplies a transaction hash
//!   (and the signer id needed to look it up) as a hint. Every material
//!   fact - execution success, target contract, method, arguments, memo,
//!   attached deposit, amount - is read from the chain by this adapter
//!   and re-verified before an observation is produced. A client
//!   assertion of payment has no path into a funding decision.
//! - Memo-bound intents. A deposit intent is bound to a unique memo
//!   (the intent id). Only a finalized token receipt, executed by the
//!   asset contract itself, whose own nep141 ft_transfer event pays the
//!   gateway account with exactly that memo can confirm the intent. A
//!   transaction can therefore never fund two intents, a fabricated hash
//!   funds nothing, and an event logged by any other contract is ignored
//!   (see the verify module).
//! - Finality. Transaction status is requested with wait_until set to
//!   FINAL, so a confirmation observation only exists for a transaction
//!   whose block and receipts are finalized.
//!
//! The funding decision itself stays in the pure aps_payment::reconcile
//! function and the idempotent ledger in aps-core: the adapter only
//! produces observations, exactly like the deterministic mock provider.

pub mod provider;
pub mod rpc;
pub mod verify;

pub use provider::{verify_tx, FetchStep, NearConfig, NearProvider, TxHint, Verification};
pub use rpc::{
    AccessKeyView, AccountView, OutcomeStatus, ReceiptOutcome, ReceiptView, RpcError, StatusInfo,
    TxAction, TxOutcome, TxStatus,
};
pub use verify::{
    check_outcome, verify_tx_receipts, OrphanTransfer, ReceiptCheck, TransferEvidence,
};

/// Re-exported so callers can build a provider against the shared trait.
pub use aps_payment::PaymentProvider;
