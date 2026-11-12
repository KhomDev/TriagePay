/* Live judge demo. Text only: nothing from the chain is parsed as HTML. */
(function () {
  "use strict";

  var reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  var state = { scenarios: [], busy: false, review: null };

  function $(id) { return document.getElementById(id); }

  function el(tag, attrs) {
    var node = document.createElement(tag);
    var children = Array.prototype.slice.call(arguments, 2);
    if (attrs) {
      Object.keys(attrs).forEach(function (key) {
        if (attrs[key] != null) node.setAttribute(key, String(attrs[key]));
      });
    }
    children.forEach(function (child) {
      if (child == null || child === false) return;
      if (Array.isArray(child)) {
        child.forEach(function (item) { if (item) node.appendChild(item); });
        return;
      }
      node.appendChild(typeof child === "string" ? document.createTextNode(child) : child);
    });
    return node;
  }

  function clear(node) {
    while (node.firstChild) node.removeChild(node.firstChild);
    return node;
  }

  function pill(text, kind) {
    var cls = kind === "allow" || kind === "success" ? "ok"
      : kind === "warn" ? "warn"
      : "bad";
    return el("span", { class: "pill " + cls }, text);
  }

  function selected() {
    var id = $("scenario").value;
    return state.scenarios.filter(function (item) { return item.id === id; })[0] || null;
  }

  function paintScenario() {
    var scenario = selected();
    var hint = clear($("scenario-hint"));
    if (!scenario) {
      $("injection").value = "";
      return;
    }
    hint.appendChild(document.createTextNode(scenario.narrative || ""));
    $("injection").value = scenario.injection || "No injected instruction. This payment stays inside the policy.";
  }

  function fillSelect(scenarios) {
    var select = $("scenario");
    var current = select.value;
    clear(select);
    scenarios.forEach(function (scenario) {
      select.appendChild(el("option", { value: scenario.id }, scenario.title));
    });
    if (current && scenarios.some(function (item) { return item.id === current; })) select.value = current;
    else if (scenarios.some(function (item) { return item.id === "injected-payee"; })) select.value = "injected-payee";
    paintScenario();
  }

  function accountLink(base, id) {
    if (!base || !id) return document.createTextNode(id || "");
    return el("a", { href: base + id, rel: "noopener", target: "_blank" }, id);
  }

  function paintSpend(data) {
    var spend = data.spend;
    var label = clear($("spend-label"));
    if (!spend) {
      label.appendChild(document.createTextNode(data.message || "Wallet views are not available."));
      return;
    }
    label.appendChild(document.createTextNode(
      "Spent today " + spend.spentDisplay + " of " + spend.dailyDisplay + ". Remaining " + spend.remainingDisplay + "."
    ));
    var percent = spend.percent.toFixed(1);
    $("spend-bar").setAttribute("aria-valuenow", percent);
    $("spend-bar-fill").style.width = percent + "%";
    $("spend-percent").textContent = percent + "%";
    var meta = clear($("spend-meta"));
    meta.appendChild(document.createTextNode(
      (spend.active ? "Agent is active. " : "Agent is revoked. Launch will restore it before paying. ") +
      (data.paused ? "Wallet is paused. " : "") +
      "Per-call cap " + spend.perCallDisplay + ". " +
      (spend.resetsAt ? "Window resets " + spend.resetsAt + ". " : "") +
      "Allowlist from " + (spend.allowlistSource === "chain" ? "the wallet contract. " : "config.json. ")
    ));
    meta.appendChild(document.createTextNode("Wallet "));
    meta.appendChild(accountLink(data.explorerAccount, data.wallet));
    meta.appendChild(document.createTextNode(". Payee "));
    meta.appendChild(accountLink(data.explorerAccount, data.payee));
    meta.appendChild(document.createTextNode("."));
    $("btn-revoke").disabled = !spend.active || !data.signing;
    $("btn-restore").disabled = spend.active || !data.signing;
  }

  function paintRisks(risks) {
    var root = clear($("risks"));
    if (!risks || !risks.length) {
      root.appendChild(el("p", { class: "hint" }, "Risk checks appear once the demo wallet is configured."));
      return;
    }
    risks.forEach(function (risk) {
      var reasons = (risk.reasons || []).map(function (item) { return item.text; }).join("; ");
      root.appendChild(el("p", null,
        pill(risk.verdict, risk.verdict),
        " ",
        el("span", { class: "mono" }, risk.accountId),
        " — ",
        reasons || "no reasons",
        risk.allowlistSource === "chain" ? " (allowlist from the chain)" : " (allowlist from config)"
      ));
    });
  }

  function paintPending(review) {
    var root = clear($("pending"));
    var copy = clear($("review-copy"));
    if (!review || !review.supported) {
      copy.appendChild(document.createTextNode(
        (review && review.reason) || "The deployed wallet does not have the approval queue yet."
      ));
      copy.appendChild(document.createTextNode(
        " The contract in this repository queues a new payee, or an amount above the owner's threshold, until the owner approves or rejects it. Deploy that wasm to make the buttons below submit a transaction. The walkthrough does not."
      ));
      $("btn-queue").disabled = true;
      return;
    }
    copy.appendChild(document.createTextNode(
      review.reviewUnknownPayees
        ? "Unknown payees are queued on chain. "
        : "Unknown payees are still refused. "
    ));
    copy.appendChild(document.createTextNode(
      "Approval threshold " + (review.approvalThreshold || "0") + ". " +
      (review.pendingCount || 0) + " waiting."
    ));
    state.queueScenario = review.reviewUnknownPayees ? "review-payee" : (BigInt(review.approvalThreshold || "0") > 0n ? "above-threshold" : null);
    $("btn-queue").textContent = state.queueScenario === "review-payee" ? "Queue a new payee" : "Queue a payment above the threshold";
    $("btn-queue").disabled = !state.queueScenario || state.busy;
    var rows = review.pending || [];
    if (!rows.length) {
      root.appendChild(el("p", { class: "hint" }, "Nothing is waiting."));
      return;
    }
    rows.forEach(function (row) {
      var id = String(row.payment_id);
      root.appendChild(el("p", null,
        pill(row.reason || "held", "warn"),
        " #",
        id,
        " ",
        el("span", { class: "mono" }, row.receiver_id || ""),
        " ",
        String(row.amount || "")
      ));
      var actions = el("div", { class: "actions" });
      var approve = el("button", { type: "button" }, "Approve " + id);
      var reject = el("button", { type: "button", class: "secondary" }, "Reject " + id);
      approve.addEventListener("click", function () { onReview("approve", id); });
      reject.addEventListener("click", function () { onReview("reject", id); });
      actions.appendChild(approve);
      actions.appendChild(reject);
      root.appendChild(actions);
    });
  }

  function paintRecent(rows) {
    var body = clear($("recent"));
    if (!rows || !rows.length) {
      body.appendChild(el("tr", null, el("td", { colspan: "5" }, "No attempts yet.")));
      return;
    }
    rows.forEach(function (row) {
      var when = row.atMs ? new Date(row.atMs).toISOString().replace("T", " ").slice(0, 19) + " UTC" : "";
      var chain = row.outcome === "success" ? "settled" : row.errorCode || row.outcome || "";
      var link = row.explorerUrl
        ? el("a", { href: row.explorerUrl, rel: "noopener", target: "_blank" }, "view")
        : document.createTextNode("");
      body.appendChild(el("tr", null,
        el("td", null, when),
        el("td", null, row.title || row.kind || ""),
        el("td", null, row.riskVerdict ? pill(row.riskVerdict, row.riskVerdict) : document.createTextNode("")),
        el("td", null, chain),
        el("td", null, link)
      ));
    });
  }

  function wordFor(payload) {
    if (!payload || !payload.chain || !payload.chain.submitted) return null;
    if (payload.chain.held) return { text: "HELD", kind: "warn" };
    if (payload.chain.outcome === "success") return { text: "SETTLED", kind: "ok" };
    return { text: "REFUSED", kind: "bad" };
  }

  function paintResult(target, payload, isError) {
    var root = clear(target);
    if (isError) {
      root.appendChild(el("p", { class: "notice bad", role: "alert" }, payload));
      return;
    }
    if (typeof payload === "string") {
      root.appendChild(el("p", null, payload));
      return;
    }
    var showWord = target.id === "verdict" || target.id === "result" || target.id === "review-result";
    var word = showWord ? wordFor(payload) : null;
    if (word) root.appendChild(el("p", { class: "verdict-word " + word.kind }, word.text));
    if (payload.summary) root.appendChild(el("p", null, payload.summary));
    if (payload.risk) {
      root.appendChild(el("p", null, "Payee risk ", pill(payload.risk.verdict, payload.risk.verdict)));
    }
    if (payload.amountCheck) {
      root.appendChild(el("p", null, "Amount check ", pill(payload.amountCheck.verdict, payload.amountCheck.verdict)));
    }
    var chain = payload.chain;
    if (chain && chain.explorerUrl) {
      root.appendChild(el("p", null, el("a", {
        href: chain.explorerUrl,
        rel: "noopener",
        target: "_blank",
      }, "Transaction on the testnet explorer")));
    }
    if (chain && chain.detail && !chain.errorCode) {
      root.appendChild(el("p", { class: "hint" }, chain.detail));
    }
  }

  function setSteps(names, bad) {
    ["step-agent", "step-instruction", "step-wallet", "step-verdict"].forEach(function (id) {
      var node = $(id);
      node.classList.remove("is-on", "is-bad");
      if (names.indexOf(id) !== -1) node.classList.add(bad && id === "step-verdict" ? "is-bad" : "is-on");
    });
  }

  function sleep(ms) {
    if (reduceMotion || ms <= 0) return Promise.resolve();
    return new Promise(function (resolve) { setTimeout(resolve, ms); });
  }

  async function api(path, body) {
    var init = { method: body ? "POST" : "GET", headers: {} };
    if (body) {
      init.headers["content-type"] = "application/json";
      init.body = JSON.stringify(body);
    }
    var response = await fetch(path, init);
    var text = await response.text();
    var parsed = null;
    try { parsed = JSON.parse(text); } catch (err) { parsed = null; }
    return { status: response.status, ok: response.ok, json: parsed, text: text };
  }

  function errorText(result) {
    if (result.json && result.json.error && result.json.error.message) return result.json.error.message;
    return result.text || ("request failed (" + result.status + ")");
  }

  async function refresh() {
    var status = $("status");
    clear(status);
    status.appendChild(document.createTextNode("Reading the wallet on testnet…"));
    var result = await api("/api/dashboard");
    clear(status);
    if (!result.ok || !result.json) {
      status.appendChild(el("span", { class: "notice bad" }, errorText(result)));
      return;
    }
    var data = result.json;
    state.scenarios = data.scenarios || [];
    state.review = data.review || null;
    fillSelect(state.scenarios);
    if (!data.configured) {
      status.appendChild(document.createTextNode(data.message || "Demo wallet is not configured."));
    } else if (!data.signing) {
      status.appendChild(document.createTextNode("Chain reads are live. Signing keys are not set on this server, so Launch stays off."));
    } else {
      status.appendChild(document.createTextNode(
        "Live on NEAR testnet. Cooldown " + data.cooldownSeconds + "s, " +
        data.maxAttemptsPerDay + " signed attempts per UTC day, " +
        data.ipLimit + " per address each " + data.ipWindowSeconds + "s. " +
        (data.sharedLimits ? "Shared store is on." : "Shared store is off, so the per-address cap is per instance and the daily cap follows the chain index.")
      ));
    }
    paintSpend(data);
    paintRisks(data.risks);
    paintPending(data.review);
    paintRecent(data.recent);
    var ready = Boolean(data.signing && data.configured);
    $("btn-launch").disabled = !ready || state.busy;
    $("btn-run").disabled = !ready || state.busy;
  }

  async function playFlow(payload, isError) {
    $("flow").hidden = false;
    setSteps(["step-agent"], false);
    await sleep(280);
    setSteps(["step-agent", "step-instruction"], false);
    await sleep(280);
    setSteps(["step-agent", "step-instruction", "step-wallet"], false);
    await sleep(280);
    var word = wordFor(payload);
    var bad = isError || (word && word.kind === "bad");
    setSteps(["step-agent", "step-instruction", "step-wallet", "step-verdict"], bad);
    paintResult($("verdict"), isError ? payload : payload, isError);
  }

  async function onLaunch() {
    if (state.busy) return;
    state.busy = true;
    $("btn-launch").disabled = true;
    clear($("verdict"));
    $("flow").hidden = false;
    setSteps(["step-agent"], false);
    try {
      var result = await api("/api/run", { scenario: "injected-payee" });
      if (!result.ok) await playFlow(errorText(result), true);
      else await playFlow(result.json, false);
    } finally {
      state.busy = false;
      await refresh();
    }
  }

  async function onRun(event) {
    event.preventDefault();
    if (state.busy) return;
    var scenario = selected();
    if (!scenario) return;
    state.busy = true;
    $("btn-run").disabled = true;
    paintResult($("result"), "Submitting on NEAR testnet…", false);
    try {
      var result = await api("/api/run", { scenario: scenario.id });
      if (!result.ok) paintResult($("result"), errorText(result), true);
      else paintResult($("result"), result.json, false);
    } finally {
      state.busy = false;
      await refresh();
    }
  }

  async function onOwner(action) {
    if (state.busy) return;
    state.busy = true;
    $("btn-revoke").disabled = true;
    $("btn-restore").disabled = true;
    paintResult($("owner-result"), action === "revoke" ? "Revoking the agent…" : "Restoring the agent…", false);
    try {
      var result = await api("/api/revoke", { action: action });
      if (!result.ok) paintResult($("owner-result"), errorText(result), true);
      else paintResult($("owner-result"), result.json, false);
    } finally {
      state.busy = false;
      await refresh();
    }
  }

  async function onReview(action, paymentId) {
    if (state.busy) return;
    state.busy = true;
    paintResult($("review-result"), action === "approve" ? "Approving on chain…" : "Rejecting on chain…", false);
    try {
      var result = await api("/api/review", { action: action, payment_id: paymentId });
      if (!result.ok) paintResult($("review-result"), errorText(result), true);
      else paintResult($("review-result"), result.json, false);
    } finally {
      state.busy = false;
      await refresh();
    }
  }

  async function onQueue() {
    if (state.busy) return;
    state.busy = true;
    paintResult($("review-result"), state.queueScenario === "above-threshold" ? "Asking the wallet to hold a payment above the threshold." : "Asking the wallet to hold a new payee…", false);
    try {
      var result = await api("/api/run", { scenario: state.queueScenario || "review-payee" });
      if (!result.ok) paintResult($("review-result"), errorText(result), true);
      else paintResult($("review-result"), result.json, false);
    } finally {
      state.busy = false;
      await refresh();
    }
  }

  async function onWalk() {
    var steps = $("review-flow").querySelectorAll(".step");
    steps.forEach(function (node) { node.classList.remove("is-on", "is-bad"); });
    var order = [0, 1, 2, 3];
    for (var i = 0; i < order.length; i++) {
      steps[i].classList.add(i === 3 ? "is-bad" : "is-on");
      await sleep(220);
    }
    paintResult($("review-result"), "Walkthrough only. No transaction was sent. On the new wallet, a warn payment waits until I approve or reject it.", false);
  }

  async function onAttest() {
    if (state.busy) return;
    state.busy = true;
    $("btn-attest").disabled = true;
    paintResult($("attest-result"), "Asking NEAR AI Cloud for an attestation report…", false);
    try {
      var result = await api("/api/judgment");
      var body = result.json || {};
      var root = clear($("attest-result"));
      root.appendChild(el("p", null, body.summary || errorText(result)));
      (body.checks || []).forEach(function (check) {
        root.appendChild(el("p", null, pill(check.verified ? "checked" : "not verified", check.verified ? "allow" : "warn"), " ", check.detail || check.name));
      });
    } finally {
      state.busy = false;
      $("btn-attest").disabled = false;
    }
  }

  $("btn-launch").addEventListener("click", onLaunch);
  $("run-form").addEventListener("submit", onRun);
  $("scenario").addEventListener("change", paintScenario);
  $("btn-refresh").addEventListener("click", function () { refresh(); });
  $("btn-revoke").addEventListener("click", function () { onOwner("revoke"); });
  $("btn-restore").addEventListener("click", function () { onOwner("restore"); });
  $("btn-queue").addEventListener("click", onQueue);
  $("btn-walk").addEventListener("click", onWalk);
  $("btn-attest").addEventListener("click", onAttest);

  refresh();
  setInterval(function () {
    if (state.busy || document.visibilityState === "hidden") return;
    refresh();
  }, 20000);
})();
