//! Sandbox integration tests for the policy wallet, over near-workspaces.
//!
//! Status: written for Linux CI. near-sandbox has no Windows build, so on
//! Windows every test is compiled and reported as ignored; none of these
//! tests has been executed on a Windows host.
//!
//! Prerequisite: build both contracts first (see docs/contracts/build.md).
//! From the `contracts/` workspace root:
//!
//! ```text
//! (cd policy-wallet && cargo near build non-reproducible-wasm --locked --no-abi)
//! (cd test-token    && cargo near build non-reproducible-wasm --locked --no-abi)
//! cargo test -p sandbox-tests
//! ```
//!
//! `POLICY_WALLET_WASM` and `TEST_TOKEN_WASM` override the artifact paths.

use aps_core::Amount;
use aps_near::rpc::{parse_tx_outcome, TxOutcome};
use aps_near::{check_outcome, Verification};
use aps_payment::{AssetId, DepositIntent, IntentId, IntentStatus};
use near_workspaces::network::Sandbox;
use near_workspaces::operations::Function;
use near_workspaces::result::ExecutionFinalResult;
use near_workspaces::types::{Gas, KeyType, NearToken, SecretKey};
use near_workspaces::{AccessKey, Account, AccountId, Contract, Worker};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const PER_CALL: u128 = 50_000; // 0.05 at 6 decimals
const DAILY: u128 = 200_000; // 0.20
const FUNDING: u128 = 1_000_000; // 1.00
const SUPPLY: u128 = 1_000_000_000;
const PAY_GAS: Gas = Gas::from_tgas(50);
const ONE_YOCTO: NearToken = NearToken::from_yoctonear(1);

fn wasm(name: &str, env_var: &str) -> Vec<u8> {
    let path = std::env::var(env_var)
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("target")
                .join("near")
                .join(name)
                .join(format!("{name}.wasm"))
        });
    std::fs::read(&path).unwrap_or_else(|e| {
        panic!(
            "cannot read {}: {e}; build the contract with cargo near first",
            path.display()
        )
    })
}

/// A deployed wallet with one agent, one funded token and one payee.
struct Env {
    worker: Worker<Sandbox>,
    root: Account,
    owner: Account,
    /// The agent signing with a function-call key limited to `pay`.
    agent: Account,
    /// The same agent account signing with its full-access key.
    agent_full: Account,
    gateway: Account,
    wallet: Account,
    token: Contract,
}

async fn sub(
    root: &Account,
    name: &str,
    near: u128,
) -> Result<Account, Box<dyn std::error::Error>> {
    Ok(root
        .create_subaccount(name)
        .initial_balance(NearToken::from_near(near))
        .transact()
        .await?
        .into_result()?)
}

async fn deploy_token(
    root: &Account,
    name: &str,
    owner: &Account,
    supply: u128,
) -> Result<Contract, Box<dyn std::error::Error>> {
    let account = sub(root, name, 10).await?;
    let token = account
        .deploy(&wasm("test_token", "TEST_TOKEN_WASM"))
        .await?
        .into_result()?;
    token
        .call("new")
        .args_json(json!({"owner_id": owner.id(), "total_supply": supply.to_string()}))
        .transact()
        .await?
        .into_result()?;
    Ok(token)
}

async fn register(token: &Contract, payer: &Account, account: &AccountId) -> TestResult {
    payer
        .call(token.id(), "storage_deposit")
        .args_json(json!({"account_id": account}))
        .deposit(NearToken::from_millinear(10))
        .transact()
        .await?
        .into_result()?;
    Ok(())
}

async fn fund(
    token: &Contract,
    from: &Account,
    to: &AccountId,
    amount: u128,
) -> Result<ExecutionFinalResult, Box<dyn std::error::Error>> {
    Ok(from
        .call(token.id(), "ft_transfer_call")
        .args_json(json!({"receiver_id": to, "amount": amount.to_string(), "msg": ""}))
        .deposit(ONE_YOCTO)
        .max_gas()
        .transact()
        .await?)
}

async fn add_function_call_key(
    worker: &Worker<Sandbox>,
    account: &Account,
    receiver: &AccountId,
    methods: &[&str],
) -> Result<Account, Box<dyn std::error::Error>> {
    let sk = SecretKey::from_random(KeyType::ED25519);
    account
        .batch(account.id())
        .add_key(
            sk.public_key(),
            AccessKey::function_call_access(receiver, methods, Some(NearToken::from_near(1))),
        )
        .transact()
        .await?
        .into_result()?;
    Ok(Account::from_secret_key(account.id().clone(), sk, worker))
}

async fn setup_with(per_call: u128, daily: u128) -> Result<Env, Box<dyn std::error::Error>> {
    let worker = near_workspaces::sandbox().await?;
    let root = worker.root_account()?;
    let owner = sub(&root, "owner", 50).await?;
    let agent_full = sub(&root, "agent", 10).await?;
    let gateway = sub(&root, "gateway", 5).await?;
    let token = deploy_token(&root, "token", &owner, SUPPLY).await?;

    // Deploy and initialize in one batch signed by the wallet account.
    // `new` accepts this account or its parent, and nobody else.
    let wallet = sub(&root, "wallet", 20).await?;
    wallet
        .batch(wallet.id())
        .deploy(&wasm("policy_wallet", "POLICY_WALLET_WASM"))
        .call(Function::new("new").args_json(json!({"owner": owner.id(), "tokens": [token.id()]})))
        .transact()
        .await?
        .into_result()?;

    register(&token, &owner, wallet.id()).await?;
    register(&token, &owner, gateway.id()).await?;
    fund(&token, &owner, wallet.id(), FUNDING)
        .await?
        .into_result()?;

    owner
        .call(wallet.id(), "set_agent")
        .args_json(json!({
            "agent_id": agent_full.id(),
            "per_call_max": per_call.to_string(),
            "daily_max": daily.to_string(),
            "payees": [gateway.id()],
        }))
        .deposit(ONE_YOCTO)
        .transact()
        .await?
        .into_result()?;

    let agent = add_function_call_key(&worker, &agent_full, wallet.id(), &["pay"]).await?;
    Ok(Env {
        worker,
        root,
        owner,
        agent,
        agent_full,
        gateway,
        wallet,
        token,
    })
}

async fn setup() -> Result<Env, Box<dyn std::error::Error>> {
    setup_with(PER_CALL, DAILY).await
}

impl Env {
    async fn pay_with(
        &self,
        signer: &Account,
        token: &AccountId,
        receiver: &AccountId,
        amount: u128,
    ) -> Result<ExecutionFinalResult, Box<dyn std::error::Error>> {
        Ok(signer
            .call(self.wallet.id(), "pay")
            .args_json(
                json!({"token": token, "receiver_id": receiver, "amount": amount.to_string()}),
            )
            .gas(PAY_GAS)
            .transact()
            .await?)
    }

    async fn pay(&self, amount: u128) -> Result<ExecutionFinalResult, Box<dyn std::error::Error>> {
        self.pay_with(&self.agent, self.token.id(), self.gateway.id(), amount)
            .await
    }

    async fn balance_on(
        &self,
        token: &AccountId,
        account: &AccountId,
    ) -> Result<u128, Box<dyn std::error::Error>> {
        let v: String = self
            .worker
            .view(token, "ft_balance_of")
            .args_json(json!({"account_id": account}))
            .await?
            .json()?;
        Ok(v.parse()?)
    }

    async fn balance(&self, account: &AccountId) -> Result<u128, Box<dyn std::error::Error>> {
        self.balance_on(self.token.id(), account).await
    }

    async fn spent(&self) -> Result<u128, Box<dyn std::error::Error>> {
        let v: String = self
            .worker
            .view(self.wallet.id(), "get_spent")
            .args_json(json!({"agent_id": self.agent.id()}))
            .await?
            .json()?;
        Ok(v.parse()?)
    }

    async fn agent_view(&self) -> Result<Value, Box<dyn std::error::Error>> {
        Ok(self
            .worker
            .view(self.wallet.id(), "get_agent")
            .args_json(json!({"agent_id": self.agent.id()}))
            .await?
            .json()?)
    }

    async fn owner_call(
        &self,
        method: &str,
        args: Value,
    ) -> Result<ExecutionFinalResult, Box<dyn std::error::Error>> {
        Ok(self
            .owner
            .call(self.wallet.id(), method)
            .args_json(args)
            .deposit(ONE_YOCTO)
            .max_gas()
            .transact()
            .await?)
    }

    /// Wallet balance, payee balance and agent spend, for "nothing moved"
    /// assertions.
    async fn snapshot(&self) -> Result<(u128, u128, u128), Box<dyn std::error::Error>> {
        Ok((
            self.balance(self.wallet.id()).await?,
            self.balance(self.gateway.id()).await?,
            self.spent().await?,
        ))
    }
}

fn has_event(res: &ExecutionFinalResult, event: &str) -> bool {
    let needle = format!(r#""event":"{event}""#);
    res.logs().iter().any(|l| {
        l.starts_with("EVENT_JSON:")
            && l.contains(r#""standard":"aps_policy_wallet""#)
            && l.contains(&needle)
    })
}

fn assert_refused(res: &ExecutionFinalResult, code: &str) {
    assert!(
        res.is_failure(),
        "expected a refusal with {code}, got success"
    );
    let failures = format!("{:?}", res.failures());
    assert!(failures.contains(code), "expected {code} in {failures}");
}

/// Refused at the transaction level (the node rejects it) or by the
/// contract: either way the call must not succeed.
fn assert_not_success(res: Result<ExecutionFinalResult, near_workspaces::error::Error>) {
    if let Ok(r) = res {
        assert!(r.is_failure(), "call unexpectedly succeeded");
    }
}

// ----- happy path ---------------------------------------------------------------

#[tokio::test]
#[cfg_attr(windows, ignore = "near-sandbox has no Windows build")]
async fn agent_pays_allowlisted_payee_with_function_call_key() -> TestResult {
    let env = setup().await?;
    let (wallet0, gateway0, _) = env.snapshot().await?;
    let res = env.pay(PER_CALL).await?;
    assert!(res.is_success(), "{:?}", res.failures());
    assert!(
        res.receipt_failures().is_empty(),
        "{:?}",
        res.receipt_failures()
    );
    assert!(has_event(&res, "pay_requested"));
    assert!(has_event(&res, "pay_settled"));
    assert_eq!(env.balance(env.gateway.id()).await?, gateway0 + PER_CALL);
    assert_eq!(env.balance(env.wallet.id()).await?, wallet0 - PER_CALL);
    assert_eq!(env.spent().await?, PER_CALL);
    Ok(())
}

// ----- limits ---------------------------------------------------------------------

#[tokio::test]
#[cfg_attr(windows, ignore = "near-sandbox has no Windows build")]
async fn per_call_limit_is_refused_and_nothing_moves() -> TestResult {
    let env = setup().await?;
    let before = env.snapshot().await?;
    assert_refused(&env.pay(PER_CALL + 1).await?, "ERR_PER_CALL_LIMIT");
    assert_eq!(env.snapshot().await?, before);
    Ok(())
}

#[tokio::test]
#[cfg_attr(windows, ignore = "near-sandbox has no Windows build")]
async fn daily_limit_is_refused_and_nothing_moves() -> TestResult {
    let env = setup().await?;
    for _ in 0..4 {
        assert!(env.pay(PER_CALL).await?.is_success());
    }
    let before = env.snapshot().await?;
    assert_eq!(before.2, DAILY);
    assert_refused(&env.pay(1).await?, "ERR_DAILY_LIMIT");
    assert_eq!(env.snapshot().await?, before);
    Ok(())
}

#[tokio::test]
#[cfg_attr(windows, ignore = "near-sandbox has no Windows build")]
async fn window_rolls_over_after_fast_forward() -> TestResult {
    let env = setup().await?;
    for _ in 0..4 {
        assert!(env.pay(PER_CALL).await?.is_success());
    }
    assert_refused(&env.pay(1).await?, "ERR_DAILY_LIMIT");
    let day0 = env.agent_view().await?["window_index"].clone();
    // Sandbox fast-forward advances block timestamps; keep going until the
    // UTC day changes (bounded so a stalled clock fails instead of hanging).
    let mut rolled = false;
    for _ in 0..40 {
        env.worker.fast_forward(10_000).await?;
        if env.agent_view().await?["window_index"] != day0 {
            rolled = true;
            break;
        }
    }
    assert!(
        rolled,
        "UTC day did not change after 400k fast-forwarded blocks"
    );
    assert_eq!(env.spent().await?, 0);
    let gateway0 = env.balance(env.gateway.id()).await?;
    assert!(env.pay(PER_CALL).await?.is_success());
    assert_eq!(env.balance(env.gateway.id()).await?, gateway0 + PER_CALL);
    assert_eq!(env.spent().await?, PER_CALL);
    Ok(())
}

#[tokio::test]
#[cfg_attr(windows, ignore = "near-sandbox has no Windows build")]
async fn concurrent_pays_cannot_exceed_the_daily_limit() -> TestResult {
    // Two pays sent at once from two function-call keys of the same agent
    // (separate keys avoid a nonce clash). Whether or not they land in one
    // block, the wallet serializes them: exactly one fits the 80k limit.
    let env = setup_with(PER_CALL, 80_000).await?;
    let second =
        add_function_call_key(&env.worker, &env.agent_full, env.wallet.id(), &["pay"]).await?;
    let gateway0 = env.balance(env.gateway.id()).await?;
    let (a, b) = tokio::join!(
        env.pay_with(&env.agent, env.token.id(), env.gateway.id(), PER_CALL),
        env.pay_with(&second, env.token.id(), env.gateway.id(), PER_CALL),
    );
    let (a, b) = (a?, b?);
    assert_eq!(
        [a.is_success(), b.is_success()]
            .iter()
            .filter(|s| **s)
            .count(),
        1,
        "exactly one pay must succeed"
    );
    let refused = if a.is_success() { &b } else { &a };
    assert_refused(refused, "ERR_DAILY_LIMIT");
    assert_eq!(env.balance(env.gateway.id()).await?, gateway0 + PER_CALL);
    assert_eq!(env.spent().await?, PER_CALL);
    Ok(())
}

#[tokio::test]
#[cfg_attr(windows, ignore = "near-sandbox has no Windows build")]
async fn overflow_adjacent_amount_uses_checked_math() -> TestResult {
    let env = setup().await?;
    let big = deploy_token(&env.root, "big", &env.owner, u128::MAX).await?;
    register(&big, &env.owner, env.wallet.id()).await?;
    register(&big, &env.owner, env.gateway.id()).await?;
    // One token per wallet: the large-supply token replaces the default.
    // It must be allowlisted before funding, or ft_on_transfer refuses it
    // and the token refunds the transfer.
    env.owner_call("set_tokens", json!({"tokens": [big.id()]}))
        .await?
        .into_result()?;
    fund(&big, &env.owner, env.wallet.id(), u128::MAX)
        .await?
        .into_result()?;
    assert_eq!(env.balance_on(big.id(), env.wallet.id()).await?, u128::MAX);
    env.owner_call(
        "set_agent",
        json!({
            "agent_id": env.agent.id(),
            "per_call_max": u128::MAX.to_string(),
            "daily_max": u128::MAX.to_string(),
            "payees": [env.gateway.id()],
        }),
    )
    .await?
    .into_result()?;
    let res = env
        .pay_with(&env.agent, big.id(), env.gateway.id(), u128::MAX - 1)
        .await?;
    assert!(res.is_success() && has_event(&res, "pay_settled"));
    assert_eq!(
        env.balance_on(big.id(), env.gateway.id()).await?,
        u128::MAX - 1
    );
    let res = env
        .pay_with(&env.agent, big.id(), env.gateway.id(), 2)
        .await?;
    assert_refused(&res, "ERR_OVERFLOW");
    assert_eq!(env.balance_on(big.id(), env.wallet.id()).await?, 1);
    Ok(())
}

// ----- allowlists, pause, revoke ------------------------------------------------

#[tokio::test]
#[cfg_attr(windows, ignore = "near-sandbox has no Windows build")]
async fn non_allowlisted_payee_is_refused() -> TestResult {
    let env = setup().await?;
    let stranger = sub(&env.root, "stranger", 5).await?;
    register(&env.token, &env.owner, stranger.id()).await?;
    let before = env.snapshot().await?;
    let res = env
        .pay_with(&env.agent, env.token.id(), stranger.id(), 1)
        .await?;
    assert_refused(&res, "ERR_PAYEE_NOT_ALLOWED");
    assert_eq!(env.snapshot().await?, before);
    assert_eq!(env.balance(stranger.id()).await?, 0);
    Ok(())
}

#[tokio::test]
#[cfg_attr(windows, ignore = "near-sandbox has no Windows build")]
async fn disallowed_token_is_refused() -> TestResult {
    let env = setup().await?;
    let other = deploy_token(&env.root, "other", &env.owner, SUPPLY).await?;
    register(&other, &env.owner, env.wallet.id()).await?;
    // Funding with a token that is not allowlisted is refused by
    // ft_on_transfer, so the token refunds the sender in full.
    let owner0 = env.balance_on(other.id(), env.owner.id()).await?;
    let res = fund(&other, &env.owner, env.wallet.id(), 1_000).await?;
    assert!(res
        .receipt_failures()
        .iter()
        .any(|f| format!("{f:?}").contains("ERR_TOKEN_NOT_ALLOWED")));
    assert_eq!(env.balance_on(other.id(), env.owner.id()).await?, owner0);
    assert_eq!(env.balance_on(other.id(), env.wallet.id()).await?, 0);
    let res = env
        .pay_with(&env.agent, other.id(), env.gateway.id(), 1)
        .await?;
    assert_refused(&res, "ERR_TOKEN_NOT_ALLOWED");
    Ok(())
}

#[tokio::test]
#[cfg_attr(windows, ignore = "near-sandbox has no Windows build")]
async fn paused_wallet_refuses_then_unpause_restores() -> TestResult {
    let env = setup().await?;
    let res = env.owner_call("pause", json!({})).await?;
    assert!(res.is_success() && has_event(&res, "paused"));
    let before = env.snapshot().await?;
    assert_refused(&env.pay(1).await?, "ERR_PAUSED");
    assert_eq!(env.snapshot().await?, before);
    let res = env.owner_call("unpause", json!({})).await?;
    assert!(res.is_success() && has_event(&res, "unpaused"));
    assert!(env.pay(1).await?.is_success());
    Ok(())
}

#[tokio::test]
#[cfg_attr(windows, ignore = "near-sandbox has no Windows build")]
async fn revoked_agent_is_refused() -> TestResult {
    let env = setup().await?;
    let res = env
        .owner_call("revoke_agent", json!({"agent_id": env.agent.id()}))
        .await?;
    assert!(res.is_success() && has_event(&res, "agent_revoked"));
    let before = env.snapshot().await?;
    assert_refused(&env.pay(1).await?, "ERR_AGENT_INACTIVE");
    assert_eq!(env.snapshot().await?, before);
    Ok(())
}

// ----- the agent key can never reach owner methods -------------------------------

#[tokio::test]
#[cfg_attr(windows, ignore = "near-sandbox has no Windows build")]
async fn agent_function_call_key_cannot_call_owner_methods() -> TestResult {
    let env = setup().await?;
    let escalate = json!({
        "agent_id": env.agent.id(),
        "per_call_max": u128::MAX.to_string(),
        "daily_max": u128::MAX.to_string(),
        "payees": [env.agent.id()],
    });
    // 1. The provisioned key is limited to `pay`: the node rejects any
    //    other method, with or without a deposit.
    let res = env
        .agent
        .call(env.wallet.id(), "set_agent")
        .args_json(escalate.clone())
        .transact()
        .await;
    assert_not_success(res);
    // 2. Even a function-call key whose method list allows every method
    //    cannot pass the owner check: with zero deposit assert_one_yocto
    //    refuses, and a function-call key cannot attach the 1 yoctoNEAR.
    let any_method =
        add_function_call_key(&env.worker, &env.agent_full, env.wallet.id(), &[]).await?;
    let res = any_method
        .call(env.wallet.id(), "set_agent")
        .args_json(escalate.clone())
        .transact()
        .await?;
    assert_refused(&res, "exactly 1 yoctoNEAR");
    let res = any_method
        .call(env.wallet.id(), "withdraw")
        .args_json(json!({"token": env.token.id(), "amount": "1", "to": env.agent.id()}))
        .deposit(ONE_YOCTO)
        .max_gas()
        .transact()
        .await;
    assert_not_success(res);
    let view = env.agent_view().await?;
    assert_eq!(view["per_call_max"], PER_CALL.to_string());
    assert_eq!(view["payees"], json!([env.gateway.id()]));
    Ok(())
}

#[tokio::test]
#[cfg_attr(windows, ignore = "near-sandbox has no Windows build")]
async fn agent_full_access_key_is_not_the_owner() -> TestResult {
    let env = setup().await?;
    let wallet0 = env.balance(env.wallet.id()).await?;
    let res = env
        .agent_full
        .call(env.wallet.id(), "withdraw")
        .args_json(json!({"token": env.token.id(), "amount": "1", "to": env.agent.id()}))
        .deposit(ONE_YOCTO)
        .max_gas()
        .transact()
        .await?;
    assert_refused(&res, "ERR_NOT_OWNER");
    assert_eq!(env.balance(env.wallet.id()).await?, wallet0);
    // pay also refuses an attached deposit from a full-access key. `pay` is
    // not #[payable], so near-sdk's generated entry point refuses the
    // deposit before the method body runs; the contract's own
    // ERR_DEPOSIT_NOT_ZERO check is reached only by a direct (unit-test)
    // call. Either way nothing moves.
    let snapshot = env.snapshot().await?;
    let res = env
        .agent_full
        .call(env.wallet.id(), "pay")
        .args_json(json!({"token": env.token.id(), "receiver_id": env.gateway.id(), "amount": "1"}))
        .deposit(ONE_YOCTO)
        .gas(PAY_GAS)
        .transact()
        .await?;
    assert_refused(&res, "doesn't accept deposit");
    assert_eq!(env.snapshot().await?, snapshot);
    Ok(())
}

// ----- downstream failure releases the reservation --------------------------------

#[tokio::test]
#[cfg_attr(windows, ignore = "near-sandbox has no Windows build")]
async fn unregistered_receiver_failure_releases_the_reservation() -> TestResult {
    let env = setup().await?;
    let unregistered = sub(&env.root, "unregistered", 5).await?;
    env.owner_call(
        "set_agent",
        json!({
            "agent_id": env.agent.id(),
            "per_call_max": PER_CALL.to_string(),
            "daily_max": DAILY.to_string(),
            "payees": [env.gateway.id(), unregistered.id()],
        }),
    )
    .await?
    .into_result()?;
    let wallet0 = env.balance(env.wallet.id()).await?;
    let res = env
        .pay_with(&env.agent, env.token.id(), unregistered.id(), PER_CALL)
        .await?;
    // The ft_transfer receipt fails; the callback runs and releases.
    assert!(!res.receipt_failures().is_empty());
    assert!(has_event(&res, "pay_requested"));
    assert!(has_event(&res, "pay_failed"));
    assert!(res
        .logs()
        .iter()
        .any(|l| l.contains(r#""release":"restored""#)));
    assert_eq!(env.spent().await?, 0);
    assert_eq!(env.balance(env.wallet.id()).await?, wallet0);
    // The full daily budget is still available.
    for _ in 0..4 {
        assert!(env.pay(PER_CALL).await?.is_success());
    }
    Ok(())
}

// ----- owner withdraw and privacy of init and callbacks ---------------------------

#[tokio::test]
#[cfg_attr(windows, ignore = "near-sandbox has no Windows build")]
async fn owner_withdraws_tokens() -> TestResult {
    let env = setup().await?;
    let owner0 = env.balance(env.owner.id()).await?;
    let wallet0 = env.balance(env.wallet.id()).await?;
    let res = env
        .owner_call(
            "withdraw",
            json!({"token": env.token.id(), "amount": "300000", "to": env.owner.id()}),
        )
        .await?;
    assert!(
        res.is_success() && has_event(&res, "withdrawn"),
        "{:?}",
        res.failures()
    );
    assert_eq!(env.balance(env.owner.id()).await?, owner0 + 300_000);
    assert_eq!(env.balance(env.wallet.id()).await?, wallet0 - 300_000);
    Ok(())
}

#[tokio::test]
#[cfg_attr(windows, ignore = "near-sandbox has no Windows build")]
async fn init_allows_the_account_or_its_parent_only() -> TestResult {
    let env = setup().await?;
    let code = wasm("policy_wallet", "POLICY_WALLET_WASM");
    let init_args = json!({"owner": env.owner.id(), "tokens": [env.token.id()]});

    // A stranger is not the account and not its parent.
    let fresh = sub(&env.root, "fresh", 10).await?;
    fresh.deploy(&code).await?.into_result()?;
    let res = env
        .owner
        .call(fresh.id(), "new")
        .args_json(init_args.clone())
        .transact()
        .await?;
    assert_refused(&res, "ERR_INIT_NOT_ALLOWED");

    // The account itself may init, in the same batch as the deploy.
    let self_init = sub(&env.root, "selfinit", 10).await?;
    let res = self_init
        .batch(self_init.id())
        .deploy(&code)
        .call(Function::new("new").args_json(init_args.clone()))
        .transact()
        .await?;
    assert!(res.is_success(), "{:?}", res.failures());
    let config: Value = env
        .worker
        .view(self_init.id(), "get_config")
        .await?
        .json()?;
    assert_eq!(config["owner"], json!(env.owner.id()));

    // The parent may init. That is the factory's predecessor in the create batch.
    let by_parent = sub(&env.root, "byparent", 10).await?;
    by_parent.deploy(&code).await?.into_result()?;
    let res = env
        .root
        .call(by_parent.id(), "new")
        .args_json(init_args)
        .transact()
        .await?;
    assert!(res.is_success(), "{:?}", res.failures());
    let config: Value = env
        .worker
        .view(by_parent.id(), "get_config")
        .await?
        .json()?;
    assert_eq!(config["owner"], json!(env.owner.id()));
    // The agent cannot forge a failed-transfer callback to refund itself.
    let res = env
        .agent_full
        .call(env.wallet.id(), "on_pay_complete")
        .args_json(json!({
            "agent_id": env.agent.id(),
            "token": env.token.id(),
            "receiver_id": env.gateway.id(),
            "amount": "1",
            "payment_id": "0",
            "window_index": "0",
        }))
        .transact()
        .await?;
    assert_refused(&res, "Method on_pay_complete is private");
    // A second init of the live wallet fails as well.
    let res = env
        .wallet
        .call(env.wallet.id(), "new")
        .args_json(json!({"owner": env.agent.id(), "tokens": []}))
        .transact()
        .await?;
    assert!(res.is_failure());
    let config: Value = env
        .worker
        .view(env.wallet.id(), "get_config")
        .await?
        .json()?;
    assert_eq!(config["owner"], json!(env.owner.id()));
    assert_eq!(config["event_standard"], "aps_policy_wallet");
    Ok(())
}

// ----- close ------------------------------------------------------------------------

#[tokio::test]
#[cfg_attr(windows, ignore = "near-sandbox has no Windows build")]
async fn close_is_owner_only_and_requires_paused() -> TestResult {
    let env = setup().await?;
    let res = env
        .agent_full
        .call(env.wallet.id(), "close")
        .args_json(json!({"beneficiary": env.agent.id()}))
        .deposit(ONE_YOCTO)
        .max_gas()
        .transact()
        .await?;
    assert_refused(&res, "ERR_NOT_OWNER");
    let res = env
        .owner_call("close", json!({"beneficiary": env.owner.id()}))
        .await?;
    assert_refused(&res, "ERR_NOT_PAUSED");
    env.worker.view_account(env.wallet.id()).await?;
    Ok(())
}

#[tokio::test]
#[cfg_attr(windows, ignore = "near-sandbox has no Windows build")]
async fn close_is_refused_while_tokens_remain() -> TestResult {
    let env = setup().await?;
    env.owner_call("pause", json!({})).await?.into_result()?;
    let res = env
        .owner_call("close", json!({"beneficiary": env.owner.id()}))
        .await?;
    assert!(has_event(&res, "close_refused"), "{:?}", res.logs());
    assert!(!has_event(&res, "closed"));
    let account = env.worker.view_account(env.wallet.id()).await?;
    assert!(account.balance > NearToken::from_near(1));
    assert_eq!(env.balance(env.wallet.id()).await?, FUNDING);
    Ok(())
}

#[tokio::test]
#[cfg_attr(windows, ignore = "near-sandbox has no Windows build")]
async fn close_after_withdrawing_everything_deletes_the_wallet() -> TestResult {
    let env = setup().await?;
    env.owner_call("pause", json!({})).await?.into_result()?;
    let remaining = env.balance(env.wallet.id()).await?;
    env.owner_call(
        "withdraw",
        json!({"token": env.token.id(), "amount": remaining.to_string(), "to": env.owner.id()}),
    )
    .await?
    .into_result()?;
    assert_eq!(env.balance(env.wallet.id()).await?, 0);
    // The beneficiary signs nothing, so no gas charge or refund of its own
    // lands in the measurement (the owner's do: it signs every call here).
    let beneficiary = sub(&env.root, "beneficiary", 1).await?;
    let wallet_near = env.worker.view_account(env.wallet.id()).await?.balance;
    let before = env.worker.view_account(beneficiary.id()).await?.balance;
    let res = env
        .owner_call("close", json!({"beneficiary": beneficiary.id()}))
        .await?;
    assert!(res.is_success(), "{:?}", res.failures());
    assert!(has_event(&res, "closed"));
    assert!(
        env.worker.view_account(env.wallet.id()).await.is_err(),
        "wallet account still exists"
    );
    let after = env.worker.view_account(beneficiary.id()).await?.balance;
    // The beneficiary receives the wallet's whole NEAR balance, stake
    // included. The wallet can only have grown since it was read (the
    // contract account earns a share of the gas its receipts burn), so
    // the gain is at least the balance read, and not much more.
    let gained = after.as_yoctonear() - before.as_yoctonear();
    let held = wallet_near.as_yoctonear();
    let tolerance = NearToken::from_millinear(50).as_yoctonear();
    assert!(
        gained >= held && gained - held < tolerance,
        "beneficiary gained {gained}, wallet held {held}"
    );
    Ok(())
}

// ----- review queue, restore, and the receipt verifier ---------------------------

async fn pay_memo(
    env: &Env,
    receiver: &AccountId,
    amount: u128,
    memo: &str,
) -> Result<ExecutionFinalResult, Box<dyn std::error::Error>> {
    Ok(env
        .agent
        .call(env.wallet.id(), "pay")
        .args_json(json!({
            "token": env.token.id(),
            "receiver_id": receiver,
            "amount": amount.to_string(),
            "memo": memo,
        }))
        .gas(PAY_GAS)
        .transact()
        .await?)
}

#[tokio::test]
#[cfg_attr(windows, ignore = "near-sandbox has no Windows build")]
async fn hold_then_approve_moves_funds_once() -> TestResult {
    let env = setup().await?;
    let stranger = sub(&env.root, "stranger", 5).await?;
    register(&env.token, &env.owner, stranger.id()).await?;
    env.owner_call(
        "set_review_policy",
        json!({"approval_threshold": "0", "review_unknown_payees": true}),
    )
    .await?
    .into_result()?;
    let wallet0 = env.balance(env.wallet.id()).await?;
    let res = pay_memo(&env, stranger.id(), 1_000, "hold-approve").await?;
    assert!(res.is_success(), "{:?}", res.failures());
    assert!(has_event(&res, "payment_queued"));
    assert!(!has_event(&res, "pay_settled"));
    assert_eq!(env.balance(env.wallet.id()).await?, wallet0);
    assert_eq!(env.balance(stranger.id()).await?, 0);
    assert_eq!(env.spent().await?, 0);
    let pending: Vec<Value> = env
        .worker
        .view(env.wallet.id(), "get_pending")
        .await?
        .json()?;
    assert_eq!(pending.len(), 1);
    let res = env
        .owner_call(
            "approve_payment",
            json!({"payment_id": pending[0]["payment_id"]}),
        )
        .await?;
    assert!(
        res.is_success() && has_event(&res, "pay_settled"),
        "{:?}",
        res.failures()
    );
    assert_eq!(env.balance(env.wallet.id()).await?, wallet0 - 1_000);
    assert_eq!(env.balance(stranger.id()).await?, 1_000);
    assert_eq!(env.spent().await?, 1_000);
    let pending: Vec<Value> = env
        .worker
        .view(env.wallet.id(), "get_pending")
        .await?
        .json()?;
    assert!(pending.is_empty());
    let again = env
        .owner_call("approve_payment", json!({"payment_id": "0"}))
        .await?;
    assert_refused(&again, "ERR_NOT_PENDING");
    assert_eq!(env.balance(stranger.id()).await?, 1_000);
    Ok(())
}

#[tokio::test]
#[cfg_attr(windows, ignore = "near-sandbox has no Windows build")]
async fn hold_then_reject_moves_nothing_and_restores_the_budget() -> TestResult {
    let env = setup().await?;
    env.owner_call(
        "set_review_policy",
        json!({"approval_threshold": "1000", "review_unknown_payees": false}),
    )
    .await?
    .into_result()?;
    let before = env.snapshot().await?;
    let res = pay_memo(&env, env.gateway.id(), 1_001, "hold-reject").await?;
    assert!(
        res.is_success() && has_event(&res, "payment_queued"),
        "{:?}",
        res.failures()
    );
    assert!(!has_event(&res, "pay_settled"));
    assert_eq!(env.balance(env.wallet.id()).await?, before.0);
    assert_eq!(env.balance(env.gateway.id()).await?, before.1);
    assert_eq!(env.spent().await?, 0);
    let res = env
        .owner_call("reject_payment", json!({"payment_id": "0"}))
        .await?;
    assert!(
        res.is_success() && has_event(&res, "payment_rejected"),
        "{:?}",
        res.failures()
    );
    assert_eq!(env.snapshot().await?, before);
    let pending: Vec<Value> = env
        .worker
        .view(env.wallet.id(), "get_pending")
        .await?
        .json()?;
    assert!(pending.is_empty());
    Ok(())
}

#[tokio::test]
#[cfg_attr(windows, ignore = "near-sandbox has no Windows build")]
async fn revoke_then_restore_keeps_the_daily_total() -> TestResult {
    let env = setup().await?;
    assert!(env.pay(PER_CALL).await?.is_success());
    env.owner_call("revoke_agent", json!({"agent_id": env.agent.id()}))
        .await?
        .into_result()?;
    assert_refused(&env.pay(1).await?, "ERR_AGENT_INACTIVE");
    env.owner_call(
        "set_agent",
        json!({
            "agent_id": env.agent.id(),
            "per_call_max": PER_CALL.to_string(),
            "daily_max": DAILY.to_string(),
            "payees": [env.gateway.id()],
        }),
    )
    .await?
    .into_result()?;
    let view = env.agent_view().await?;
    assert_eq!(view["active"], true);
    assert_eq!(view["spent_in_window"], PER_CALL.to_string());
    let gateway0 = env.balance(env.gateway.id()).await?;
    assert!(env.pay(PER_CALL).await?.is_success());
    assert_eq!(env.balance(env.gateway.id()).await?, gateway0 + PER_CALL);
    assert_eq!(env.spent().await?, PER_CALL * 2);
    Ok(())
}

#[tokio::test]
#[cfg_attr(windows, ignore = "near-sandbox has no Windows build")]
async fn pay_only_key_cannot_call_review_methods() -> TestResult {
    let env = setup().await?;
    let before = env.snapshot().await?;
    assert_not_success(
        env.agent
            .call(env.wallet.id(), "set_review_policy")
            .args_json(json!({"approval_threshold": "1", "review_unknown_payees": true}))
            .transact()
            .await,
    );
    assert_not_success(
        env.agent
            .call(env.wallet.id(), "approve_payment")
            .args_json(json!({"payment_id": "0"}))
            .transact()
            .await,
    );
    assert_not_success(
        env.agent
            .call(env.wallet.id(), "reject_payment")
            .args_json(json!({"payment_id": "0"}))
            .transact()
            .await,
    );
    assert_eq!(env.snapshot().await?, before);
    let review: Value = env
        .worker
        .view(env.wallet.id(), "get_review_policy")
        .await?
        .json()?;
    assert_eq!(review["approval_threshold"], "0");
    assert_eq!(review["review_unknown_payees"], false);
    Ok(())
}

/// The sandbox node answers `EXPERIMENTAL_tx_status`. The adapter's first
/// choice, `tx_status`, is a later method and this node rejects it.
fn sandbox_tx_outcome(url: &str, tx_hash: &str, signer_id: &str) -> TxOutcome {
    let body = json!({
        "jsonrpc": "2.0",
        "id": "sandbox",
        "method": "EXPERIMENTAL_tx_status",
        "params": {
            "tx_hash": tx_hash,
            "sender_account_id": signer_id,
            "wait_until": "FINAL",
        },
    });
    let response = ureq::post(url)
        .set("Content-Type", "application/json")
        .send_string(&body.to_string())
        .unwrap_or_else(|err| panic!("sandbox rpc: {err}"))
        .into_string()
        .expect("sandbox rpc body");
    let parsed: Value = serde_json::from_str(&response).expect("sandbox rpc json");
    if let Some(err) = parsed.get("error") {
        panic!("sandbox rpc error: {err}");
    }
    parse_tx_outcome(tx_hash, &parsed["result"]).expect("sandbox transaction status")
}

#[tokio::test]
#[cfg_attr(windows, ignore = "near-sandbox has no Windows build")]
async fn x402_verifier_accepts_the_token_event_and_ignores_a_moved_log() -> TestResult {
    let env = setup().await?;
    let memo = "intent-sandbox-1";
    let amount = 7_000u128;
    let res = pay_memo(&env, env.gateway.id(), amount, memo).await?;
    assert!(
        res.is_success() && has_event(&res, "pay_settled"),
        "{:?}",
        res.failures()
    );
    let tx_hash = res.outcome().transaction_hash.to_string();
    let outcome = sandbox_tx_outcome(
        env.worker.rpc_addr().trim(),
        &tx_hash,
        env.agent.id().as_str(),
    );
    let intent = DepositIntent {
        intent_id: IntentId::new(memo.to_string()),
        deposit_address: env.gateway.id().to_string(),
        asset_id: AssetId::new(env.token.id().as_str()),
        amount: Amount::from_atomic(amount),
        expires_at_unix: 10_000,
        status: IntentStatus::Pending,
    };
    let check = check_outcome(&intent, &outcome);
    match &check.verification {
        Verification::Confirmed { amount_atomic, .. } => {
            assert_eq!(amount_atomic, &amount.to_string());
        }
        other => panic!("expected a confirmed receipt, got {other:?}"),
    }
    let evidence = check.evidence.expect("confirmed payments carry evidence");
    assert_eq!(evidence.predecessor_id, env.wallet.id().as_str());
    assert_eq!(evidence.payer_id, env.wallet.id().as_str());
    assert_eq!(evidence.receiver_id, env.gateway.id().as_str());
    assert_eq!(evidence.memo, memo);
    assert_eq!(evidence.executor_id, env.token.id().as_str());

    let mut spoofed = outcome.clone();
    let mut moved = false;
    for receipt in &mut spoofed.receipts_outcome {
        if receipt.executor_id == env.token.id().as_str() {
            receipt.executor_id = env.wallet.id().to_string();
            moved = true;
        }
    }
    assert!(moved, "the token receipt was not in the status");
    let spoof = check_outcome(&intent, &spoofed);
    assert!(
        matches!(spoof.verification, Verification::StructuralMismatch(_)),
        "a log moved off the token executor must not confirm: {:?}",
        spoof.verification
    );
    assert!(spoof.evidence.is_none());
    Ok(())
}

/// A hijacked agent used to fill the review queue and the daily cap.
/// Holds must not spend the cap, and one agent cannot take every slot.
#[tokio::test]
#[cfg_attr(windows, ignore = "near-sandbox has no Windows build")]
async fn hijacked_agent_cannot_fill_the_queue_or_the_daily_budget() -> TestResult {
    let env = setup().await?;
    env.owner_call(
        "set_review_policy",
        json!({"approval_threshold": "0", "review_unknown_payees": true}),
    )
    .await?
    .into_result()?;
    let before = env.snapshot().await?;
    for i in 0..3 {
        let stranger = sub(&env.root, &format!("junk{i}"), 2).await?;
        let res = env
            .pay_with(&env.agent, env.token.id(), stranger.id(), 1)
            .await?;
        assert!(res.is_success(), "{:?}", res.failures());
        assert!(has_event(&res, "payment_queued"));
    }
    assert_eq!(
        env.snapshot().await?,
        before,
        "holds must not move balances or spend"
    );
    let fourth = sub(&env.root, "junk3", 2).await?;
    assert_refused(
        &env.pay_with(&env.agent, env.token.id(), fourth.id(), 1)
            .await?,
        "ERR_AGENT_OPEN_HOLDS",
    );
    assert_eq!(env.snapshot().await?, before);
    let settled = env.pay(PER_CALL).await?;
    assert!(settled.is_success(), "{:?}", settled.failures());
    assert_eq!(env.spent().await?, PER_CALL);
    let res = env.owner_call("reject_all", json!({})).await?;
    assert!(res.is_success(), "{:?}", res.failures());
    assert!(has_event(&res, "payment_rejected"));
    assert_eq!(env.spent().await?, PER_CALL);
    let pending: Vec<Value> = env
        .worker
        .view(env.wallet.id(), "get_pending")
        .await?
        .json()?;
    assert!(pending.is_empty());
    Ok(())
}
