import {
  getTokens,
  nearDepositCall,
  quoteRequest,
  quoteStatus,
  requestQuote,
  submitDeposit,
  toAtomic,
  usdcToken,
} from "./intents.js";

const $ = (id) => document.getElementById(id);
const say = (text) => {
  $("topup-out").textContent = text;
};

function selectedToken() {
  const id = $("origin").value;
  return (window.__tokens || []).find((token) => token.assetId === id) || null;
}

function accounts() {
  const recipient = $("wallet").value.trim();
  const refundTo = $("refund").value.trim();
  if (!recipient || !refundTo) throw new Error("the policy wallet and the refund account are both required");
  return { recipient, refundTo };
}

async function loadTokens() {
  say("Loading the token list from 1Click…");
  const tokens = await getTokens();
  window.__tokens = tokens;
  const usdc = usdcToken(tokens);
  if (!usdc) {
    say("GET /v0/tokens did not list Circle USDC on NEAR. I will not guess a destination asset.");
    return;
  }
  const origin = $("origin");
  origin.replaceChildren();
  const near = tokens.filter((token) => token.blockchain === "near" && token.assetId !== usdc.assetId);
  const other = tokens.filter((token) => token.blockchain !== "near");
  for (const [label, list] of [["NEAR", near], ["Other chains", other]]) {
    const group = document.createElement("optgroup");
    group.label = label;
    for (const token of list) {
      const option = document.createElement("option");
      option.value = token.assetId;
      option.textContent = token.symbol + " · " + token.blockchain;
      group.append(option);
    }
    origin.append(group);
  }
  say("USDC destination is " + usdc.assetId + ". Pick a token you already hold.");
}

async function quote(dry) {
  const token = selectedToken();
  const usdc = usdcToken(window.__tokens || []);
  if (!token || !usdc) throw new Error("the token list is not loaded");
  const { recipient, refundTo } = accounts();
  const amount = toAtomic($("amount").value, token.decimals);
  const body = quoteRequest({
    originAsset: token.assetId,
    destinationAsset: usdc.assetId,
    amount,
    recipient,
    refundTo,
    dry,
  });
  const result = await requestQuote(body);
  const quoted = result.quote || {};
  const lines = [
    dry ? "Preview. No deposit address, because dry is true." : "Live quote. Send the deposit or it expires.",
    "In: " + (quoted.amountInFormatted || amount) + " " + token.symbol,
    "Out, about: " + (quoted.amountOutFormatted || "?") + " USDC",
    "Minimum out: " + (quoted.minAmountOut || "?"),
  ];
  if (!dry && quoted.depositAddress) {
    window.__deposit = { address: quoted.depositAddress, memo: quoted.depositMemo || "", amount, token };
    lines.push("Deposit address: " + quoted.depositAddress);
    if (quoted.depositMemo) lines.push("Memo: " + quoted.depositMemo);
    if (token.blockchain === "near" && token.contractAddress) {
      const call = nearDepositCall({
        contract: token.contractAddress,
        depositAddress: quoted.depositAddress,
        amount,
        memo: quoted.depositMemo,
      });
      lines.push("");
      lines.push("Sign this ft_transfer in your own wallet. Attach 1 yoctoNEAR. This page does not open a wallet and does not see a key.");
      lines.push(JSON.stringify(call, null, 2));
    } else {
      lines.push("");
      lines.push("This token is on " + token.blockchain + ". Send the amount to the deposit address from a wallet on that chain. I do not build that chain's transaction.");
    }
  }
  say(lines.join("\n"));
}

async function sendHash() {
  const deposit = window.__deposit;
  const txHash = $("tx").value.trim();
  if (!deposit || !txHash) throw new Error("create a deposit address, sign the transfer, then paste the transaction hash");
  const status = await submitDeposit({
    depositAddress: deposit.address,
    txHash,
    nearSenderAccount: $("refund").value.trim(),
    memo: deposit.memo,
  });
  say("Submitted.\n" + JSON.stringify(status, null, 2));
}

async function refresh() {
  const deposit = window.__deposit;
  if (!deposit) throw new Error("there is no deposit address yet");
  const status = await quoteStatus(deposit.address);
  say(JSON.stringify(status, null, 2));
}

function bind(id, fn) {
  $(id).addEventListener("click", () => {
    fn().catch((error) => say(error && error.message ? error.message : String(error)));
  });
}

$("topup").addEventListener("submit", (event) => event.preventDefault());
bind("load-tokens", loadTokens);
bind("preview", () => quote(true));
bind("deposit", () => quote(false));
bind("submit-tx", sendHash);
bind("status", refresh);
loadTokens().catch((error) => say(error && error.message ? error.message : String(error)));
