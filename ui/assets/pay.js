/* Pay on NEAR: the x402 flow step by step, and a read-only tx checker. */
(function () {
  "use strict";
  var A = window.APS;
  var S = window.APSSamples;
  var el = A.el;

  function $(id) { return document.getElementById(id); }

  function b64json(value) {
    var bytes = new TextEncoder().encode(JSON.stringify(value));
    var binary = "";
    bytes.forEach(function (b) { binary += String.fromCharCode(b); });
    return btoa(binary);
  }

  function pre(value) {
    return el("pre", null, typeof value === "string" ? value : JSON.stringify(value, null, 2));
  }

  function step(title, state, body) {
    return el("li", null, el("h3", null, title, " ",
      el("span", { class: "pill " + state.cls + " state" }, state.label)), body);
  }

  function renderFlow() {
    var x = S.mockX402;
    var out = A.clear($("flow"));
    var header = el("p", null, "Data source: ", A.sourceBadge("mock"));
    out.appendChild(header);
    out.appendChild(el("p", { class: "notice mock" },
      "Live x402 routes land with the x402 build; until then this walkthrough uses " +
      "illustrative values. Accounts ending in .example.testnet and the transaction hash do " +
      "not exist on chain, so no explorer link is offered for them."));
    var done = { cls: "ok", label: "done" };
    var list = el("ol", { class: "steps" },
      step("Request without payment: HTTP 402", done, [
        el("p", null, "The client calls the paid resource. The gateway answers 402 with a ",
          el("code", null, "PAYMENT-REQUIRED"), " header: base64 JSON listing what it accepts. ",
          "The memo binds the payment to one server-issued intent."),
        el("p", { class: "hint" }, "Header value"),
        el("pre", { class: "hash" }, b64json(x.paymentRequired)),
        el("p", { class: "hint" }, "Decoded"),
        pre(x.paymentRequired)
      ]),
      step("Pay on NEAR: one token transfer", done, [
        el("p", null, "The payer sends exactly the required amount of the settlement token to ",
          "the gateway, with the intent id as memo. NEP-141 transfers attach exactly 1 ",
          "yoctoNEAR, so this needs a full-access key or the policy wallet's pay method."),
        A.kv([
          ["Signer", x.transfer.signer_id, "mono"],
          ["Token contract", x.transfer.receiver_id, "mono"],
          ["Method", x.transfer.method, "mono"],
          ["Attached deposit", x.transfer.deposit_yocto + " yoctoNEAR"],
          ["Transaction", x.transfer.tx_hash, "mono"],
          ["Explorer", "for a real hash: " + A.explorerTx("testnet", "<tx hash>"), "mono hash"]
        ]),
        pre(x.transfer.args)
      ]),
      step("Retry with proof: PAYMENT-SIGNATURE", done, [
        el("p", null, "The client repeats the request with a ", el("code", null,
          "PAYMENT-SIGNATURE"), " header carrying the transaction hash, signer and intent id. ",
          "This client-submitted proof method is a proposed profile, not the upstream NEAR ",
          "exact mechanism."),
        el("pre", { class: "hash" }, b64json(x.paymentPayload)),
        pre(x.paymentPayload)
      ]),
      step("Verification against the chain", done, [
        el("p", null, "The gateway reads the transaction from NEAR RPC and checks, before any ",
          "work runs:"),
        el("ul", null, x.verification.map(function (line) { return el("li", null, line); }))
      ]),
      step("Receipt: PAYMENT-RESPONSE and the ledger", done, [
        el("p", null, "The resource runs, the ledger records the settlement against the report ",
          "hash, and the response carries a ", el("code", null, "PAYMENT-RESPONSE"), " header."),
        el("pre", { class: "hash" }, b64json(x.settlementResponse)),
        pre(x.settlementResponse)
      ])
    );
    out.appendChild(list);
  }

  function describeStatus(status) {
    if (!status) return "unknown";
    if (Object.prototype.hasOwnProperty.call(status, "SuccessValue")) return "success";
    if (Object.prototype.hasOwnProperty.call(status, "SuccessReceiptId")) return "success (receipt)";
    if (status.Failure) return "failure";
    return JSON.stringify(status);
  }

  async function checkTx(event) {
    event.preventDefault();
    var out = A.clear($("tx-out"));
    var network = $("tx-network").value;
    var hash = $("tx-hash").value.trim();
    var sender = $("tx-sender").value.trim();
    if (!/^[1-9A-HJ-NP-Za-km-z]{43,44}$/.test(hash)) {
      out.appendChild(el("p", { class: "notice bad", role: "alert" },
        "A NEAR transaction hash is 43 or 44 base58 characters."));
      return;
    }
    if (!sender) {
      out.appendChild(el("p", { class: "notice bad", role: "alert" },
        "Enter the signer account id."));
      return;
    }
    var button = $("btn-tx");
    button.disabled = true;
    try {
      var params = { tx_hash: hash, sender_account_id: sender, wait_until: "NONE" };
      var archival = false;
      var result;
      try {
        result = await A.rpc(network, "tx", params);
      } catch (err) {
        if (err.cause_name !== "UNKNOWN_TRANSACTION") throw err;
        archival = true;
        result = await A.rpc(network, "tx", params, true);
      }
      var tx = result.transaction || {};
      var methods = (tx.actions || []).map(function (action) {
        if (action.FunctionCall) return action.FunctionCall.method_name;
        return typeof action === "string" ? action : Object.keys(action)[0];
      });
      var logs = [];
      (result.receipts_outcome || []).forEach(function (receipt) {
        (receipt.outcome.logs || []).forEach(function (log) {
          logs.push({ executor: receipt.outcome.executor_id, line: log });
        });
      });
      out.appendChild(el("p", null, "Read from NEAR ", network,
        archival ? " archival RPC " : " RPC ", A.sourceBadge("live")));
      out.appendChild(A.kv([
        ["Status", describeStatus(result.status)],
        ["Finality", result.final_execution_status || "-"],
        ["Signer", tx.signer_id || "-", "mono"],
        ["Receiver", tx.receiver_id || "-", "mono"],
        ["Actions", methods.join(", ") || "-", "mono"],
        ["Explorer", el("a", { href: A.explorerTx(network, hash), rel: "noopener",
          target: "_blank" }, "open on nearblocks (new tab)")]
      ]));
      out.appendChild(el("h3", null, "Receipt logs and NEP-297 events"));
      if (!logs.length) {
        out.appendChild(el("p", { class: "hint" }, "No logs in this transaction."));
      }
      logs.forEach(function (entry) {
        var shown = entry.line;
        if (shown.indexOf("EVENT_JSON:") === 0) {
          try {
            shown = JSON.stringify(JSON.parse(shown.slice("EVENT_JSON:".length)), null, 2);
          } catch (err) {
            shown = entry.line;
          }
        }
        out.appendChild(el("p", { class: "hint mono" }, entry.executor));
        out.appendChild(pre(shown));
      });
    } catch (err) {
      out.appendChild(el("p", { class: "notice bad", role: "alert" },
        "RPC lookup failed: " + err.message));
    } finally {
      button.disabled = false;
    }
  }

  A.ready.then(function () {
    renderFlow();
    $("tx-form").addEventListener("submit", checkTx);
  });
})();
