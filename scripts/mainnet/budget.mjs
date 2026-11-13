// Create-cost and mainnet-script budget. No network, no key, no near-api-js.
//
// One NEAR locks 100_000 bytes (10^19 yoctoNEAR per byte). The factory
// account locks its own code. Each wallet locks a second copy of the
// policy-wallet wasm, plus state, plus a working balance. The 1 NEAR fee
// returns to the owner when the owner is the fee recipient. `close` returns
// the wallet's code, state, and working balance.

export const BYTE_YOCTO = 10_000_000_000_000_000_000n;
export const WALLET_CODE_BYTES = 221_906n;
export const WALLET_CODE_SHA256 =
  "fbc50fd34d703db5886c2484ff064938213fd5a591b27336300fc9592f5cda18";
export const STATE_BYTES = 1_000n;
export const WORKING_YOCTO = 300_000_000_000_000_000_000_000n;
export const FEE_YOCTO = 1_000_000_000_000_000_000_000_000n;
export const FACTORY_STATE_BYTES = 5_000n;
// Burns that do not come back, plus gas the signer must hold while the
// transaction is in flight. 300 TGas is what create_wallet asks for;
// mainnet charges the pessimistic price, about 0.001 NEAR per TGas, and
// refunds what the call does not burn.
export const DEPLOY_BURN_YOCTO = 80_000_000_000_000_000_000_000n;
export const CREATE_PREPAID_GAS_YOCTO = 300_000_000_000_000_000_000_000n;
export const FLOW_AFTER_FEE_YOCTO = 450_000_000_000_000_000_000_000n;
export const BUDGET_CEILING_YOCTO = 6_500_000_000_000_000_000_000_000n;

export const near = (yocto) => Number(yocto) / 1e24;

export function stakeYocto(codeBytes = WALLET_CODE_BYTES) {
  return BigInt(codeBytes) * BYTE_YOCTO + STATE_BYTES * BYTE_YOCTO + WORKING_YOCTO;
}

export function createCostYocto(codeBytes = WALLET_CODE_BYTES) {
  return FEE_YOCTO + stakeYocto(codeBytes);
}

// Minimum the owner must hold before the script starts. The factory fund
// leaves first. The create deposit and its prepaid gas must still be liquid
// after that. The fee comes back before the agent, the token storage, and
// the rest of the flow, so those are not added on top.
export function projectBudget(factoryBytes, codeBytes = WALLET_CODE_BYTES) {
  const factoryFund = (BigInt(factoryBytes) + FACTORY_STATE_BYTES) * BYTE_YOCTO;
  const stake = stakeYocto(codeBytes);
  const create = FEE_YOCTO + stake;
  const peak = factoryFund + DEPLOY_BURN_YOCTO + create + CREATE_PREPAID_GAS_YOCTO;
  return {
    factoryBytes: BigInt(factoryBytes),
    factoryFund,
    codeBytes: BigInt(codeBytes),
    code: BigInt(codeBytes) * BYTE_YOCTO,
    state: STATE_BYTES * BYTE_YOCTO,
    working: WORKING_YOCTO,
    stake,
    fee: FEE_YOCTO,
    create,
    deployBurn: DEPLOY_BURN_YOCTO,
    createPrepaidGas: CREATE_PREPAID_GAS_YOCTO,
    flowAfterFee: FLOW_AFTER_FEE_YOCTO,
    peak,
    ceiling: BUDGET_CEILING_YOCTO,
    fits: peak <= BUDGET_CEILING_YOCTO,
    secondWallet: false,
  };
}

export function budgetLines(b) {
  const row = (label, yocto) => `${label.padEnd(28)} ${near(yocto).toFixed(5)} NEAR`;
  return [
    `policy-wallet wasm          ${b.codeBytes} bytes`,
    `factory wasm                ${b.factoryBytes} bytes`,
    row("wallet code stake", b.code),
    row("wallet state stake", b.state),
    row("working balance", b.working),
    row("refundable storage", b.stake),
    row("fee (returns to owner)", b.fee),
    row("create_wallet deposit", b.create),
    row("factory fund", b.factoryFund),
    row("factory deploy burn", b.deployBurn),
    row("create prepaid gas", b.createPrepaidGas),
    row("peak before the fee returns", b.peak),
    row("ceiling", b.ceiling),
    `within ceiling             ${b.fits ? "yes" : "no, short " + near(b.peak - b.ceiling).toFixed(5) + " NEAR"}`,
    row("flow after the fee returns", b.flowAfterFee),
    "second wallet               skipped (another create is the deposit above; the fee covers the flow, not a second copy)",
  ];
}
