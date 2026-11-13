//! Factory for TriagePay policy wallets.
//!
//! `create_wallet(name)` is payable. It requires a 1 NEAR service fee plus
//! the NEAR the new account locks: one copy of the policy-wallet wasm, the
//! initial state, and a working balance so later holds can be stored. Any
//! excess is refunded. The fee is paid to `fee_recipient` only after `new`
//! succeeds. If creation fails, the callback refunds the fee and the stake,
//! so the user receives the whole attachment back (gas aside).
//!
//! Each wallet is `<name>.<factory>`. The batch is `create_account`,
//! `transfer`, `deploy_contract` of the embedded wasm, and `new`.
//!
//! The wallet owner recovers that NEAR with the wallet's own `close`: pause
//! the wallet, withdraw every token, then `close(beneficiary)`. Account
//! deletion returns the code stake, the state stake, and the working
//! balance.
//!
//! Global contracts (`deploy_global_contract` / `use_global_contract`) stay
//! behind the `global-contracts` feature and are off by default.
//!
//! The factory is deployed to `triagepay.hlpok1lad.tg`. This crate does not
//! deploy itself.

pub mod accounting;

#[cfg(test)]
mod tests;

use accounting::{required_deposit, wallet_account_id, QuoteError};
#[cfg(feature = "global-contracts")]
use near_sdk::json_types::Base64VecU8;
use near_sdk::json_types::U128;
use near_sdk::{assert_one_yocto, env, near, AccountId, Gas, NearToken, PanicOnDefault, Promise};

/// Service fee charged for each wallet, in yoctoNEAR. 1 NEAR.
pub const SERVICE_FEE: u128 = 1_000_000_000_000_000_000_000_000;
/// Default account that receives [`SERVICE_FEE`].
pub const DEFAULT_FEE_RECIPIENT: &str = "hlpok1lad.tg";
/// Account this factory is deployed to.
pub const FACTORY_ACCOUNT: &str = "triagepay.hlpok1lad.tg";
/// Circle native USDC on NEAR mainnet. See `policy-wallet::USDC_MAINNET`.
pub const USDC_MAINNET: &str = "17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1";
/// NEP-141 wrapper for native NEAR.
pub const WRAP_NEAR: &str = "wrap.near";

/// YoctoNEAR locked per byte of account state on mainnet.
pub const STORAGE_BYTE_COST: u128 = 10_000_000_000_000_000_000;
/// Bytes of state a fresh wallet locks, on top of its code. Measured in
/// sandbox for a long name: 281 bytes. 1_000 bytes covers a 64-character
/// owner and the 64-character USDC account id.
pub const WALLET_STATE_BYTES: u64 = 1_000;
/// Liquid NEAR left on the new wallet so an agent and a few holds can be
/// stored. Without it the third hold fails with `LackBalanceForState`.
pub const WORKING_BALANCE: u128 = 300_000_000_000_000_000_000_000;

/// Length of the wasm-opt `-Oz` policy-wallet binary that `create_wallet` deploys.
pub const WALLET_CODE_LEN: u64 = 222_412;
/// sha256 of that binary. The factory stores it compressed and inflates it
/// in `create_wallet`, so the factory account does not lock a second full copy.
/// Built with `CARGO_HOME`, `RUSTUP_HOME`, `/usr/local/cargo`, `/usr/local/rustup`,
/// and the `contracts/` directory remapped. See the README build command.
pub const WALLET_CODE_SHA256: &str =
    "f8464e1c325715c7b51311eff86bb9d995129ee3ca04d4f44b1b1ef20afa4e1d";
#[cfg(not(feature = "global-contracts"))]
const WALLET_CODE_ZLIB: &[u8] = include_bytes!("../res/policy_wallet.wasm.zlib");

/// Gas for the new wallet's `new`.
const GAS_INIT: Gas = Gas::from_tgas(40);
/// Gas for the factory callback.
const GAS_CALLBACK: Gas = Gas::from_tgas(30);
/// Gas for publishing the global contract.
#[cfg(feature = "global-contracts")]
const GAS_PUBLISH: Gas = Gas::from_tgas(50);
/// Minimum gas the caller must attach to `create_wallet`. Deploying the
/// wasm needs the headroom above the init call and the callback.
const GAS_MIN_CREATE: Gas = Gas::from_tgas(250);

const ERR_NOT_OWNER: &str = "ERR_NOT_OWNER";
const ERR_PAUSED: &str = "ERR_PAUSED";
#[cfg(feature = "global-contracts")]
const ERR_CODE_UNSET: &str = "ERR_CODE_UNSET";
const ERR_BAD_NAME: &str = "ERR_BAD_NAME";
const ERR_INSUFFICIENT_DEPOSIT: &str = "ERR_INSUFFICIENT_DEPOSIT";
const ERR_BAD_FEE: &str = "ERR_BAD_FEE";
const ERR_TOO_MANY_TOKENS: &str = "ERR_TOO_MANY_TOKENS";
#[cfg(feature = "global-contracts")]
const ERR_BAD_HASH: &str = "ERR_BAD_HASH";
#[cfg(feature = "global-contracts")]
const ERR_BAD_CODE: &str = "ERR_BAD_CODE";
const ERR_INSUFFICIENT_GAS: &str = "ERR_INSUFFICIENT_GAS";

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct WalletFactory {
    owner: AccountId,
    fee: u128,
    fee_recipient: AccountId,
    /// sha256 of the published policy-wallet Wasm. `None` until publish.
    code_hash: Option<[u8; 32]>,
    paused: bool,
    default_tokens: Vec<AccountId>,
    wallets_created: u64,
}

#[near(serializers = [json])]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateCostView {
    /// Service fee, in yoctoNEAR.
    pub fee: U128,
    /// Wasm stake, in yoctoNEAR. Zero when `global-contracts` is enabled.
    pub code: U128,
    /// Initial-state stake, in yoctoNEAR.
    pub state: U128,
    /// Liquid NEAR left on the wallet for later state growth.
    pub working: U128,
    /// `code + state + working`. `close` on the wallet returns this.
    pub storage: U128,
    /// Exact deposit `create_wallet` requires. Any more is refunded.
    pub total: U128,
    /// True while `create_wallet` is refused.
    pub paused: bool,
    /// Account that receives the fee after a successful create.
    pub fee_recipient: AccountId,
    /// Embedded wasm length, or zero when creation uses a global contract.
    pub code_bytes: u64,
    /// Hex sha256 of a published global contract, or null.
    pub code_hash_hex: Option<String>,
}

#[near(serializers = [json])]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FactoryConfigView {
    pub owner: AccountId,
    pub fee: U128,
    pub fee_recipient: AccountId,
    pub paused: bool,
    pub default_tokens: Vec<AccountId>,
    pub wallets_created: u64,
    pub code_hash_hex: Option<String>,
    pub storage_bytes: u64,
    pub code_bytes: u64,
    pub working_balance: U128,
}

#[near(event_json(standard = "triagepay_factory"))]
pub enum FactoryEvent {
    /// A wallet account was created and initialized.
    #[event_version("1.0.0")]
    WalletCreated {
        wallet_id: AccountId,
        owner: AccountId,
        fee: U128,
        storage: U128,
    },
    /// Creation failed and the user was refunded fee + storage.
    #[event_version("1.0.0")]
    WalletCreationFailed {
        wallet_id: AccountId,
        owner: AccountId,
        refunded: U128,
    },
}

fn fail(code: &str, description: &str) -> ! {
    env::panic_str(&format!("{code}: {description}"))
}

fn byte_cost(bytes: u64) -> u128 {
    u128::from(bytes) * STORAGE_BYTE_COST
}

/// Code bytes deployed onto each wallet. Zero when the global-contract
/// feature is on, because the wallet then stores only a hash reference.
pub fn wallet_code_bytes() -> u64 {
    #[cfg(feature = "global-contracts")]
    {
        0
    }
    #[cfg(not(feature = "global-contracts"))]
    {
        WALLET_CODE_LEN
    }
}

/// The wasm deployed onto each wallet. Empty when `global-contracts` is on.
pub fn embedded_wallet_code() -> Vec<u8> {
    #[cfg(feature = "global-contracts")]
    {
        Vec::new()
    }
    #[cfg(not(feature = "global-contracts"))]
    {
        miniz_oxide::inflate::decompress_to_vec_zlib(WALLET_CODE_ZLIB)
            .unwrap_or_else(|_| env::panic_str("ERR_BAD_CODE: wallet wasm"))
    }
}

/// NEAR locked on the new wallet: code, initial state, and working balance.
pub fn stake_yocto() -> u128 {
    byte_cost(wallet_code_bytes())
        .saturating_add(byte_cost(WALLET_STATE_BYTES))
        .saturating_add(WORKING_BALANCE)
}

#[cfg(feature = "global-contracts")]
fn parse_hash(hex_hash: &str) -> [u8; 32] {
    if hex_hash.len() != 64 || !hex_hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        fail(ERR_BAD_HASH, "code hash must be 64 hex characters");
    }
    let mut out = [0u8; 32];
    for i in 0..32 {
        out[i] = u8::from_str_radix(&hex_hash[i * 2..i * 2 + 2], 16)
            .unwrap_or_else(|_| fail(ERR_BAD_HASH, "code hash must be 64 hex characters"));
    }
    out
}

fn hex_hash(hash: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for byte in hash {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

#[near]
impl WalletFactory {
    /// Initialize the factory. Callable once, by the factory account.
    #[init]
    #[private]
    pub fn new(owner: AccountId, fee_recipient: AccountId, default_tokens: Vec<AccountId>) -> Self {
        if default_tokens.len() > 8 {
            fail(ERR_TOO_MANY_TOKENS, "at most 8 default tokens");
        }
        Self {
            owner,
            fee: SERVICE_FEE,
            fee_recipient,
            code_hash: None,
            paused: false,
            default_tokens,
            wallets_created: 0,
        }
    }

    /// Create `<name>.<this factory>` owned by the predecessor.
    ///
    /// Attach at least [`CreateCostView::total`] from [`get_create_cost`].
    /// Any extra is refunded. The 1 NEAR fee is sent to `fee_recipient`
    /// only after the wallet's `new` succeeds. A failed creation refunds
    /// the fee and the storage as well.
    #[payable]
    pub fn create_wallet(&mut self, name: String) -> Promise {
        if self.paused {
            fail(ERR_PAUSED, "wallet creation is paused");
        }
        if env::prepaid_gas() < GAS_MIN_CREATE {
            fail(ERR_INSUFFICIENT_GAS, "attach at least 250 TGas");
        }
        #[cfg(feature = "global-contracts")]
        let hash = self
            .code_hash
            .unwrap_or_else(|| fail(ERR_CODE_UNSET, "publish the wallet code first"));
        let wallet_id: AccountId = wallet_account_id(env::current_account_id().as_str(), &name)
            .unwrap_or_else(|_| fail(ERR_BAD_NAME, "name is not a valid wallet name"))
            .parse()
            .unwrap_or_else(|_| fail(ERR_BAD_NAME, "name is not a valid account id"));
        let storage = stake_yocto();
        let fee = self.fee;
        let cost = match required_deposit(fee, storage) {
            Ok(cost) => cost,
            Err(QuoteError::Overflow) => fail(ERR_BAD_FEE, "fee plus storage overflows"),
            Err(QuoteError::Short) => unreachable!("required_deposit does not return Short"),
        };
        let attached = env::attached_deposit().as_yoctonear();
        if attached < cost {
            fail(
                ERR_INSUFFICIENT_DEPOSIT,
                &format!("attach at least {cost} yoctoNEAR"),
            );
        }
        let excess = attached - cost;
        let user = env::predecessor_account_id();
        if excess > 0 {
            Promise::new(user.clone())
                .transfer(NearToken::from_yoctonear(excess))
                .detach();
        }
        let args = near_sdk::serde_json::json!({
            "owner": user,
            "tokens": self.default_tokens,
        })
        .to_string()
        .into_bytes();
        let created = Promise::new(wallet_id.clone())
            .create_account()
            .transfer(NearToken::from_yoctonear(storage));
        #[cfg(feature = "global-contracts")]
        let created = created.use_global_contract(hash);
        #[cfg(not(feature = "global-contracts"))]
        let created = created.deploy_contract(embedded_wallet_code());
        created
            .function_call("new", args, NearToken::from_yoctonear(0), GAS_INIT)
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(GAS_CALLBACK)
                    .on_wallet_created(user, wallet_id, U128(fee), U128(storage)),
            )
    }

    /// Callback for [`create_wallet`]. On success, pay the fee. On failure,
    /// refund fee + storage to the user. Never panics.
    #[private]
    pub fn on_wallet_created(
        &mut self,
        user: AccountId,
        wallet_id: AccountId,
        fee: U128,
        storage: U128,
    ) {
        let failed = matches!(
            env::promise_result_checked(0, 0),
            Err(near_sdk::PromiseError::Failed)
        );
        if failed {
            let refund = fee.0.saturating_add(storage.0);
            if refund > 0 {
                Promise::new(user.clone())
                    .transfer(NearToken::from_yoctonear(refund))
                    .detach();
            }
            FactoryEvent::WalletCreationFailed {
                wallet_id,
                owner: user,
                refunded: U128(refund),
            }
            .emit();
            return;
        }
        self.wallets_created = self.wallets_created.saturating_add(1);
        if fee.0 > 0 {
            Promise::new(self.fee_recipient.clone())
                .transfer(NearToken::from_yoctonear(fee.0))
                .detach();
        }
        FactoryEvent::WalletCreated {
            wallet_id,
            owner: user,
            fee,
            storage,
        }
        .emit();
    }

    /// Publish `code` as a global contract by hash and remember that hash.
    /// The burn (10x storage) is paid by this factory account.
    #[cfg(feature = "global-contracts")]
    #[payable]
    pub fn publish_wallet_code(&mut self, code: Base64VecU8) -> Promise {
        self.assert_owner();
        if code.0.is_empty() {
            fail(ERR_BAD_CODE, "wallet code is empty");
        }
        let digest = env::sha256(&code.0);
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&digest);
        Promise::new(env::current_account_id())
            .deploy_global_contract(code.0)
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(GAS_PUBLISH)
                    .on_code_published(hash.to_vec()),
            )
    }

    /// Store the hash after a successful publish. A failed publish changes
    /// nothing.
    #[cfg(feature = "global-contracts")]
    #[private]
    pub fn on_code_published(&mut self, hash: Vec<u8>) {
        if hash.len() != 32 {
            return;
        }
        if matches!(
            env::promise_result_checked(0, 0),
            Err(near_sdk::PromiseError::Failed)
        ) {
            return;
        }
        let mut code_hash = [0u8; 32];
        code_hash.copy_from_slice(&hash);
        self.code_hash = Some(code_hash);
    }

    /// Point creation at an already published global-contract hash.
    #[cfg(feature = "global-contracts")]
    #[payable]
    pub fn update_wallet_code_hash(&mut self, code_hash_hex: String) {
        self.assert_owner();
        self.code_hash = Some(parse_hash(&code_hash_hex));
    }

    /// Change the service fee. It must not overflow when added to storage.
    #[payable]
    pub fn set_fee(&mut self, fee: U128) {
        self.assert_owner();
        if required_deposit(fee.0, stake_yocto()).is_err() {
            fail(ERR_BAD_FEE, "fee plus storage overflows");
        }
        self.fee = fee.0;
    }

    /// Change who receives the service fee.
    #[payable]
    pub fn set_fee_recipient(&mut self, fee_recipient: AccountId) {
        self.assert_owner();
        if fee_recipient == env::current_account_id() {
            fail(ERR_BAD_FEE, "the factory cannot be its own fee recipient");
        }
        self.fee_recipient = fee_recipient;
    }

    /// Refuse `create_wallet` until [`resume_creation`].
    #[payable]
    pub fn pause_creation(&mut self) {
        self.assert_owner();
        self.paused = true;
    }

    /// Allow `create_wallet` again.
    #[payable]
    pub fn resume_creation(&mut self) {
        self.assert_owner();
        self.paused = false;
    }

    /// Exact deposit for one wallet: the 1 NEAR fee plus refundable storage
    /// (code, initial state, and the working balance).
    pub fn get_create_cost(&self) -> CreateCostView {
        let code = byte_cost(wallet_code_bytes());
        let state = byte_cost(WALLET_STATE_BYTES);
        let working = WORKING_BALANCE;
        let storage = code.saturating_add(state).saturating_add(working);
        let total = required_deposit(self.fee, storage).unwrap_or(u128::MAX);
        CreateCostView {
            fee: U128(self.fee),
            code: U128(code),
            state: U128(state),
            working: U128(working),
            storage: U128(storage),
            total: U128(total),
            paused: self.paused,
            fee_recipient: self.fee_recipient.clone(),
            code_bytes: wallet_code_bytes(),
            code_hash_hex: self.code_hash.as_ref().map(hex_hash),
        }
    }

    /// Factory configuration.
    pub fn get_config(&self) -> FactoryConfigView {
        FactoryConfigView {
            owner: self.owner.clone(),
            fee: U128(self.fee),
            fee_recipient: self.fee_recipient.clone(),
            paused: self.paused,
            default_tokens: self.default_tokens.clone(),
            wallets_created: self.wallets_created,
            code_hash_hex: self.code_hash.as_ref().map(hex_hash),
            storage_bytes: WALLET_STATE_BYTES,
            code_bytes: wallet_code_bytes(),
            working_balance: U128(WORKING_BALANCE),
        }
    }

    /// The account id `create_wallet` would use, or null when `name` is invalid.
    pub fn get_wallet_id(&self, name: String) -> Option<String> {
        wallet_account_id(env::current_account_id().as_str(), &name).ok()
    }

    fn assert_owner(&self) {
        assert_one_yocto();
        if env::predecessor_account_id() != self.owner {
            fail(ERR_NOT_OWNER, "only the owner can call this method");
        }
    }
}
