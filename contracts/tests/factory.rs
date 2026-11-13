//! Sandbox tests for the wallet factory.
//!
//! The factory publishes the policy wallet once, as a global contract by
//! code hash, and each wallet calls `use_global_contract`. A new wallet
//! pays for its state, not for a second copy of the Wasm.
//!
//! Prerequisite: release wasm for both contracts.
//!
//! ```text
//! cargo build -p policy-wallet -p wallet-factory --target wasm32-unknown-unknown --release
//! cargo test -p sandbox-tests --test factory
//! ```

use near_workspaces::operations::Function;
use near_workspaces::result::ExecutionFinalResult;
use near_workspaces::types::{Gas, KeyType, NearToken, SecretKey};
use near_workspaces::{AccessKey, Account, AccountId, Contract, Worker};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const BYTE_COST: u128 = 10_000_000_000_000_000_000;
const ONE_YOCTO: NearToken = NearToken::from_yoctonear(1);
const SERVICE_FEE: u128 = 1_000_000_000_000_000_000_000_000;

fn wasm(crate_name: &str) -> Vec<u8> {
    let env_var = match crate_name {
        "policy_wallet" => "POLICY_WALLET_WASM",
        "wallet_factory" => "FACTORY_WASM",
        "test_token" => "TEST_TOKEN_WASM",
        _ => "",
    };
    let path = std::env::var(env_var)
        .ok()
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("target")
                .join("wasm32-unknown-unknown")
                .join("release")
                .join(format!("{crate_name}.wasm"))
        });
    std::fs::read(&path).unwrap_or_else(|e| {
        panic!(
            "cannot read {}: {e}; build with cargo build -p {crate_name} --target wasm32-unknown-unknown --release",
            path.display()
        )
    })
}

fn b64(data: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    let mut i = 0;
    while i + 3 <= data.len() {
        let n = ((data[i] as u32) << 16) | ((data[i + 1] as u32) << 8) | data[i + 2] as u32;
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        out.push(T[((n >> 6) & 63) as usize] as char);
        out.push(T[(n & 63) as usize] as char);
        i += 3;
    }
    if i < data.len() {
        let mut n = (data[i] as u32) << 16;
        out.push(T[((n >> 18) & 63) as usize] as char);
        if i + 1 < data.len() {
            n |= (data[i + 1] as u32) << 8;
            out.push(T[((n >> 12) & 63) as usize] as char);
            out.push(T[((n >> 6) & 63) as usize] as char);
            out.push('=');
        } else {
            out.push(T[((n >> 12) & 63) as usize] as char);
            out.push('=');
            out.push('=');
        }
    }
    out
}

fn yocto(token: NearToken) -> u128 {
    token.as_yoctonear()
}

async fn sub(root: &Account, name: &str, near: u128) -> Result<Account, Box<dyn std::error::Error>> {
    Ok(root
        .create_subaccount(name)
        .initial_balance(NearToken::from_near(near))
        .transact()
        .await?
        .into_result()?)
}

fn assert_refused(res: &ExecutionFinalResult, code: &str) {
    assert!(res.is_failure(), "expected {code}, got success");
    let failures = format!("{:?}", res.failures());
    assert!(failures.contains(code), "expected {code} in {failures}");
}

struct World {
    worker: Worker<near_workspaces::network::Sandbox>,
    root: Account,
    owner: Account,
    user: Account,
    other: Account,
    factory: Account,
    usdc: Contract,
    wnear: Contract,
}

async fn deploy_token(
    root: &Account,
    name: &str,
    owner: &Account,
) -> Result<Contract, Box<dyn std::error::Error>> {
    let account = sub(root, name, 20).await?;
    let token = account.deploy(&wasm("test_token")).await?.into_result()?;
    token
        .call("new")
        .args_json(json!({"owner_id": owner.id(), "total_supply": "1000000000000"}))
        .transact()
        .await?
        .into_result()?;
    Ok(token)
}

async fn world() -> Result<World, Box<dyn std::error::Error>> {
    let worker = near_workspaces::sandbox().await?;
    let root = worker.root_account()?;
    let owner = sub(&root, "owner", 40).await?;
    let user = sub(&root, "alice", 10).await?;
    let other = sub(&root, "bob", 10).await?;
    let usdc = deploy_token(&root, "usdc", &owner).await?;
    let wnear = deploy_token(&root, "wnear", &owner).await?;

    // The factory account must be able to burn the global-contract fee.
    // Sandbox root can fund it; this is not the mainnet budget.
    let factory_account = sub(&root, "factory", 80).await?;
    let before = yocto(worker.view_account(factory_account.id()).await?.balance);
    let deployed = factory_account
        .batch(factory_account.id())
        .deploy(&wasm("wallet_factory"))
        .call(
            Function::new("new").args_json(json!({
                "owner": owner.id(),
                "fee_recipient": owner.id(),
                "default_tokens": [usdc.id(), wnear.id()],
            })),
        )
        .transact()
        .await?
        .into_result()?;
    let _ = deployed;
    let after = yocto(worker.view_account(factory_account.id()).await?.balance);
    let factory_lock = before.saturating_sub(after);
    eprintln!(
        "MEASURE factory deploy locked about {factory_lock} yocto ({:.6} NEAR), wasm {} bytes",
        factory_lock as f64 / 1e24,
        wasm("wallet_factory").len()
    );
    let factory = factory_account;
    Ok(World {
        worker,
        root,
        owner,
        user,
        other,
        factory,
        usdc,
        wnear,
    })
}

impl World {
    async fn publish(&self) -> Result<ExecutionFinalResult, Box<dyn std::error::Error>> {
        let code = wasm("policy_wallet");
        eprintln!(
            "MEASURE wallet wasm {} bytes; global burn at 10x storage is {:.6} NEAR",
            code.len(),
            code.len() as f64 * 10.0 * 1e19 / 1e24
        );
        let before = yocto(self.worker.view_account(self.factory.id()).await?.balance);
        let res = self
            .owner
            .call(self.factory.id(), "publish_wallet_code")
            .args_json(json!({"code": b64(&code)}))
            .deposit(ONE_YOCTO)
            .max_gas()
            .transact()
            .await?;
        let after = yocto(self.worker.view_account(self.factory.id()).await?.balance);
        eprintln!(
            "MEASURE publish balance drop {:.6} NEAR (includes gas), success {}",
            before.saturating_sub(after) as f64 / 1e24,
            res.is_success()
        );
        Ok(res)
    }

    async fn cost(&self) -> Result<u128, Box<dyn std::error::Error>> {
        let view: Value = self
            .worker
            .view(self.factory.id(), "get_create_cost")
            .await?
            .json()?;
        Ok(view["total"].as_str().unwrap().parse()?)
    }

    async fn create(
        &self,
        signer: &Account,
        name: &str,
        deposit: u128,
    ) -> Result<ExecutionFinalResult, Box<dyn std::error::Error>> {
        Ok(signer
            .call(self.factory.id(), "create_wallet")
            .args_json(json!({"name": name}))
            .deposit(NearToken::from_yoctonear(deposit))
            .gas(Gas::from_tgas(200))
            .transact()
            .await?)
    }
}

#[tokio::test]
#[cfg_attr(windows, ignore = "near-sandbox has no Windows build")]
async fn global_contract_wallet_costs_state_not_code() -> TestResult {
    let world = world().await?;
    let published = world.publish().await?;
    assert!(published.is_success(), "{:?}", published.failures());
    let config: Value = world
        .worker
        .view(world.factory.id(), "get_config")
        .await?
        .json()?;
    assert!(config["code_hash_hex"].as_str().unwrap().len() == 64);

    let parent = world.factory.id().as_str();
    let room = 64 - 1 - parent.len();
    let name = "n".repeat(room.min(32));
    let total = world.cost().await?;
    let storage = total - SERVICE_FEE;
    assert!(
        storage < 50_000_000_000_000_000_000_000,
        "per-wallet storage {storage} yocto is not under 0.05 NEAR"
    );

    let owner_before = yocto(world.worker.view_account(world.owner.id()).await?.balance);
    let user_before = yocto(world.worker.view_account(world.user.id()).await?.balance);
    let created = world.create(&world.user, &name, total).await?;
    assert!(created.is_success(), "{:?}", created.failures());
    let owner_after = yocto(world.worker.view_account(world.owner.id()).await?.balance);
    let user_after = yocto(world.worker.view_account(world.user.id()).await?.balance);
    let fee_gain = owner_after - owner_before;
    assert_eq!(fee_gain, SERVICE_FEE, "fee recipient gained {fee_gain}, want {SERVICE_FEE}");
    let user_spent = user_before - user_after;
    assert!(
        user_spent > total && user_spent < total + NearToken::from_millinear(200).as_yoctonear(),
        "user spent {user_spent}, deposit was {total}"
    );

    let wallet_id: AccountId = format!("{name}.{parent}").parse()?;
    let details = world.worker.view_account(&wallet_id).await?;
    let used = details.storage_usage;
    let state_cost = used as u128 * BYTE_COST;
    eprintln!(
        "MEASURE wallet storage_usage {used} bytes = {:.6} NEAR; quoted storage {:.6} NEAR",
        state_cost as f64 / 1e24,
        storage as f64 / 1e24
    );
    assert!(
        used < 5_000,
        "wallet state is {used} bytes, over the 0.05 NEAR aim"
    );
    assert!(
        state_cost <= storage,
        "quoted storage {storage} does not cover measured {state_cost}"
    );
    assert!(
        yocto(details.balance) >= state_cost,
        "wallet balance does not cover its storage"
    );
    let view: Value = world
        .worker
        .view(&wallet_id, "get_config")
        .args_json(json!({}))
        .await?
        .json()?;
    assert_eq!(view["owner"].as_str().unwrap(), world.user.id().as_str());
    assert_eq!(view["tokens"].as_array().unwrap().len(), 2);

    // Excess comes back. A second create of the same name refunds the whole deposit.
    let extra = NearToken::from_near(1).as_yoctonear();
    let other_name = "beta";
    let before = yocto(world.worker.view_account(world.other.id()).await?.balance);
    let res = world.create(&world.other, other_name, total + extra).await?;
    assert!(res.is_success(), "{:?}", res.failures());
    let after = yocto(world.worker.view_account(world.other.id()).await?.balance);
    let spent = before - after;
    assert!(
        spent + extra > total && spent < total + NearToken::from_millinear(200).as_yoctonear(),
        "excess was not refunded: spent {spent}, deposit {}",
        total + extra
    );
    let owner_mid = yocto(world.worker.view_account(world.owner.id()).await?.balance);
    let again_before = yocto(world.worker.view_account(world.other.id()).await?.balance);
    let dup = world.create(&world.other, other_name, total + extra).await?;
    assert!(dup.is_success() || dup.is_failure());
    // Creation fails inside the promise. The transaction itself can succeed
    // while the callback refunds. The user's net loss must be gas only.
    let again_after = yocto(world.worker.view_account(world.other.id()).await?.balance);
    let loss = again_before - again_after;
    assert!(
        loss < NearToken::from_millinear(300).as_yoctonear(),
        "failed create kept funds: loss {loss}"
    );
    let owner_end = yocto(world.worker.view_account(world.owner.id()).await?.balance);
    assert_eq!(
        owner_end, owner_mid,
        "a failed create paid the fee recipient"
    );

    let refused = world
        .user
        .call(world.factory.id(), "set_fee")
        .args_json(json!({"fee": "1"}))
        .deposit(ONE_YOCTO)
        .transact()
        .await?;
    assert_refused(&refused, "ERR_NOT_OWNER");
    let _ = world.root;
    Ok(())
}

#[tokio::test]
#[cfg_attr(windows, ignore = "near-sandbox has no Windows build")]
async fn created_wallet_pays_holds_and_rejects_a_queue_fill() -> TestResult {
    let world = world().await?;
    let published = world.publish().await?;
    assert!(published.is_success(), "{:?}", published.failures());
    let total = world.cost().await?;
    let created = world.create(&world.user, "mine", total).await?;
    assert!(created.is_success(), "{:?}", created.failures());
    let wallet: AccountId = format!("mine.{}", world.factory.id()).parse()?;

    for token in [world.usdc.id(), world.wnear.id()] {
        world
            .owner
            .call(token, "storage_deposit")
            .args_json(json!({"account_id": &wallet}))
            .deposit(NearToken::from_millinear(10))
            .transact()
            .await?
            .into_result()?;
        world
            .owner
            .call(token, "ft_transfer_call")
            .args_json(json!({"receiver_id": &wallet, "amount": "1000000", "msg": ""}))
            .deposit(ONE_YOCTO)
            .max_gas()
            .transact()
            .await?
            .into_result()?;
    }

    let agent = world
        .user
        .create_subaccount("agent")
        .initial_balance(NearToken::from_near(1))
        .transact()
        .await?
        .into_result()?;
    let sk = SecretKey::from_random(KeyType::ED25519);
    agent
        .batch(agent.id())
        .add_key(
            sk.public_key(),
            AccessKey::function_call_access(wallet.clone(), &["pay"], Some(NearToken::from_near(1))),
        )
        .transact()
        .await?
        .into_result()?;
    let pay_key = Account::from_secret_key(agent.id().clone(), sk, &world.worker);
    let gateway = sub(&world.root, "gate", 2).await?;
    world
        .owner
        .call(world.usdc.id(), "storage_deposit")
        .args_json(json!({"account_id": gateway.id()}))
        .deposit(NearToken::from_millinear(10))
        .transact()
        .await?
        .into_result()?;

    world
        .user
        .call(&wallet, "set_agent")
        .args_json(json!({
            "agent_id": agent.id(),
            "per_call_max": "0",
            "daily_max": "0",
            "payees": [gateway.id()],
        }))
        .deposit(ONE_YOCTO)
        .transact()
        .await?
        .into_result()?;
    world
        .user
        .call(&wallet, "set_token_limits")
        .args_json(json!({
            "agent_id": agent.id(),
            "token": world.usdc.id(),
            "per_call_max": "50000",
            "daily_max": "200000",
        }))
        .deposit(ONE_YOCTO)
        .transact()
        .await?
        .into_result()?;
    world
        .user
        .call(&wallet, "set_token_limits")
        .args_json(json!({
            "agent_id": agent.id(),
            "token": world.wnear.id(),
            "per_call_max": "50000",
            "daily_max": "200000",
        }))
        .deposit(ONE_YOCTO)
        .transact()
        .await?
        .into_result()?;

    let paid = pay_key
        .call(&wallet, "pay")
        .args_json(json!({
            "token": world.usdc.id(),
            "receiver_id": gateway.id(),
            "amount": "1000",
            "memo": null,
        }))
        .gas(Gas::from_tgas(80))
        .transact()
        .await?;
    assert!(paid.is_success(), "{:?}", paid.failures());

    world
        .user
        .call(&wallet, "set_review_policy")
        .args_json(json!({"approval_threshold": "0", "review_unknown_payees": true}))
        .deposit(ONE_YOCTO)
        .transact()
        .await?
        .into_result()?;
    let stranger = sub(&world.root, "stranger", 2).await?;
    let held = pay_key
        .call(&wallet, "pay")
        .args_json(json!({
            "token": world.usdc.id(),
            "receiver_id": stranger.id(),
            "amount": "1000",
            "memo": null,
        }))
        .gas(Gas::from_tgas(80))
        .transact()
        .await?;
    assert!(held.is_success(), "{:?}", held.failures());
    world
        .owner
        .call(world.usdc.id(), "storage_deposit")
        .args_json(json!({"account_id": stranger.id()}))
        .deposit(NearToken::from_millinear(10))
        .transact()
        .await?
        .into_result()?;
    let approved = world
        .user
        .call(&wallet, "approve_payment")
        .args_json(json!({"payment_id": "1"}))
        .deposit(ONE_YOCTO)
        .gas(Gas::from_tgas(80))
        .transact()
        .await?;
    assert!(approved.is_success(), "{:?}", approved.failures());

    let held2 = pay_key
        .call(&wallet, "pay")
        .args_json(json!({
            "token": world.wnear.id(),
            "receiver_id": stranger.id(),
            "amount": "500",
            "memo": null,
        }))
        .gas(Gas::from_tgas(80))
        .transact()
        .await?;
    assert!(held2.is_success(), "{:?}", held2.failures());
    let rejected = world
        .user
        .call(&wallet, "reject_payment")
        .args_json(json!({"payment_id": "2"}))
        .deposit(ONE_YOCTO)
        .transact()
        .await?;
    assert!(rejected.is_success(), "{:?}", rejected.failures());

    world
        .user
        .call(&wallet, "revoke_agent")
        .args_json(json!({"agent_id": agent.id()}))
        .deposit(ONE_YOCTO)
        .transact()
        .await?
        .into_result()?;
    let revoked = pay_key
        .call(&wallet, "pay")
        .args_json(json!({
            "token": world.usdc.id(),
            "receiver_id": gateway.id(),
            "amount": "1",
            "memo": null,
        }))
        .gas(Gas::from_tgas(80))
        .transact()
        .await?;
    assert_refused(&revoked, "ERR_AGENT_INACTIVE");

    world
        .user
        .call(&wallet, "set_token_limits")
        .args_json(json!({
            "agent_id": agent.id(),
            "token": world.usdc.id(),
            "per_call_max": "50000",
            "daily_max": "200000",
        }))
        .deposit(ONE_YOCTO)
        .transact()
        .await?
        .into_result()?;

    for i in 0..3 {
        let junk = sub(&world.root, &format!("j{i}"), 1).await?;
        let res = pay_key
            .call(&wallet, "pay")
            .args_json(json!({
                "token": world.usdc.id(),
                "receiver_id": junk.id(),
                "amount": "1",
                "memo": null,
            }))
            .gas(Gas::from_tgas(80))
            .transact()
            .await?;
        assert!(res.is_success(), "{:?}", res.failures());
    }
    let overflow = sub(&world.root, "j3", 1).await?;
    let blocked = pay_key
        .call(&wallet, "pay")
        .args_json(json!({
            "token": world.usdc.id(),
            "receiver_id": overflow.id(),
            "amount": "1",
            "memo": null,
        }))
        .gas(Gas::from_tgas(80))
        .transact()
        .await?;
    assert_refused(&blocked, "ERR_AGENT_OPEN_HOLDS");
    let still = pay_key
        .call(&wallet, "pay")
        .args_json(json!({
            "token": world.usdc.id(),
            "receiver_id": gateway.id(),
            "amount": "1000",
            "memo": null,
        }))
        .gas(Gas::from_tgas(80))
        .transact()
        .await?;
    assert!(still.is_success(), "{:?}", still.failures());

    let withdrawn = world
        .user
        .call(&wallet, "withdraw")
        .args_json(json!({
            "token": world.wnear.id(),
            "amount": "1000000",
            "to": world.user.id(),
        }))
        .deposit(ONE_YOCTO)
        .gas(Gas::from_tgas(80))
        .transact()
        .await?;
    assert!(withdrawn.is_success(), "{:?}", withdrawn.failures());
    Ok(())
}
