/* Live judge demo. Text only: nothing from the chain is parsed as HTML. */
(function () {
  "use strict";

  var reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  var state = { scenarios: [], busy: false, review: null, queueScenario: null };

  function $(id) { return document.getElementById(id); }

  function el(tag, attrs) {
    var node = document.createElement(tag);
    var children = Array.prototype.slice.call(arguments, 2);
    if (attrs) {
      Object.keys(attrs).forEach(function (key) {
        if (key === "class") node.className = String(attrs[key]);
        else if (attrs[key] != null) node.setAttribute(key, String(attrs[key]));
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
    while (node && node.firstChild) node.removeChild(node.firstChild);
    return node;
  }

  function pill(text, kind) {
    var cls = kind === "allow" || kind === "success" ? "ok" : kind === "warn" ? "warn" : "bad";
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
    var injected = scenarios.filter(function (item) { return item.id === "injected-payee"; })[0];
    if (injected && injected.injection) $("inject-line").textContent = injected.injection;
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
      $("stat-spent").textContent = "—";
      $("stat-remaining").textContent = "—";
      $("stat-agent").textContent = "—";
      return;
    }
    $("stat-spent").textContent = spend.spentDisplay;
    $("stat-remaining").textContent = spend.remainingDisplay;
    $("stat-agent").textContent = spend.active ? "Active" : "Revoked";
    label.appendChild(document.createTextNode("of " + spend.dailyDisplay + " today"));
    var percent = Number(spend.percent || 0);
    $("spend-bar").setAttribute("aria-valuenow", String(percent));
    $("spend-bar-fill").style.width = Math.max(0, Math.min(100, percent)) + "%";
    $("spend-percent").textContent = percent.toFixed(1) + "%";
    var meta = clear($("spend-meta"));
    meta.appendChild(document.createTextNode(
      (spend.active ? "Agent is active. " : "Agent is revoked. Launch restores it before paying. ") +
      (data.paused ? "Wallet is paused. " : "") +
      "Per-call cap " + spend.perCallDisplay + ". "
    ));
    if (data.wallet) {
      meta.appendChild(document.createTextNode("Wallet "));
      meta.appendChild(accountLink(data.explorerAccount, data.wallet));
      meta.appendChild(document.createTextNode("."));
    }
    var pending = data.review && data.review.pendingCount;
    $("stat-pending").textContent = data.review && data.review.supported ? String(pending || 0) : "—";
    $("btn-revoke").disabled = !spend.active || !data.signing || state.busy;
    $("btn-restore").disabled = spend.active || !data.signing || state.busy;
    if (data.wallet && data.explorerAccount) {
      $("nav-wallet").textContent = data.wallet;
      $("nav-wallet").setAttribute("href", data.explorerAccount + data.wallet);
    }
    $("agent-mode").textContent = data.agentMode === "live"
      ? "Agent: live · " + (data.model || "NEAR AI")
      : "Agent: scripted";
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
      $("btn-queue").disabled = true;
      return;
    }
    copy.appendChild(document.createTextNode(
      review.reviewUnknownPayees
        ? "Unknown payees are queued on chain. "
        : "Unknown payees are still refused. "
    ));
    copy.appendChild(document.createTextNode(
      "Approval threshold " + (review.approvalThresholdDisplay || review.approvalThreshold || "0") + ". " + (review.pendingCount || 0) + " waiting."
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
    if (payload.chain.outcome === "success" && !payload.chain.errorCode) return { text: "SETTLED", kind: "ok" };
    if (payload.action === "approve" && payload.chain.outcome === "success") return { text: "APPROVED", kind: "ok" };
    if (payload.action === "reject" && payload.chain.outcome === "success") return { text: "REJECTED", kind: "bad" };
    return { text: "REFUSED", kind: "bad" };
  }

  function shareCard(receipt) {
    if (!receipt) return null;
    var params = new URLSearchParams();
    params.set("kind", receipt.kind || "refused");
    params.set("rule", receipt.rule || "");
    params.set("payee", receipt.payee || "");
    params.set("amount", receipt.amount || "");
    params.set("tx", receipt.txHash || "");
    params.set("tried", (receipt.tried || "").slice(0, 140));
    var image = "/api/og?" + params.toString();
    var page = "/api/share?" + params.toString();
    var text = receipt.shareText || "";
    if (text.indexOf("built at NEAR HACKATHON @NEARProtocol #NEARHackathon") === -1) {
      text = (text ? text + " " : "") + "built at NEAR HACKATHON @NEARProtocol #NEARHackathon";
    }
    var intent = "https://x.com/intent/post?text=" + encodeURIComponent(text) + "&url=" + encodeURIComponent(location.origin + page);
    var rows = el("dl", null,
      el("dt", null, "Rule"), el("dd", null, (receipt.rule || "") + (receipt.ruleText ? " — " + receipt.ruleText : "")),
      el("dt", null, "Amount"), el("dd", null, receipt.amount || "—"),
      el("dt", null, "Payee"), el("dd", { class: "mono" }, receipt.payee || "—")
    );
    return el("article", { class: "receipt" },
      el("p", { class: "kicker" }, "On-chain receipt"),
      el("h3", null, receipt.headline || "RECEIPT"),
      el("p", null, receipt.tried || ""),
      rows,
      receipt.explorerUrl ? el("p", null, el("a", { href: receipt.explorerUrl, rel: "noopener", target: "_blank" }, "Transaction")) : null,
      el("div", { class: "actions" },
        el("a", { class: "text-btn ghost", href: image, target: "_blank", rel: "noopener" }, "Save card"),
        el("a", { class: "text-btn ghost", href: intent, target: "_blank", rel: "noopener" }, "Share on X"),
        receipt.txHash ? el("a", { class: "text-btn ghost", href: "/replay/" + receipt.txHash }, "Replay") : null
      )
    );
  }

  function paintResult(target, payload, isError) {
    var root = clear(target);
    if (!root) return;
    if (isError) {
      root.appendChild(el("p", { class: "notice bad", role: "alert" }, payload));
      return;
    }
    if (typeof payload === "string") {
      root.appendChild(el("p", null, payload));
      return;
    }
    var word = wordFor(payload);
    if (word) root.appendChild(el("p", { class: "verdict-word " + word.kind }, word.text));
    if (payload.agent && payload.agent.label) {
      root.appendChild(el("p", null, "Agent ", pill(payload.agent.label, payload.agent.mode === "live" ? "allow" : "warn")));
    }
    if (payload.summary) root.appendChild(el("p", null, payload.summary));
    if (payload.risk) root.appendChild(el("p", null, "Payee risk ", pill(payload.risk.verdict, payload.risk.verdict)));
    if (payload.amountCheck) root.appendChild(el("p", null, "Amount check ", pill(payload.amountCheck.verdict, payload.amountCheck.verdict)));
    var chain = payload.chain;
    if (chain && chain.explorerUrl) {
      root.appendChild(el("p", null, el("a", { href: chain.explorerUrl, rel: "noopener", target: "_blank" }, "Transaction on the testnet explorer")));
    }
    if (payload.telegram && payload.telegram.reason && !payload.telegram.sent) {
      root.appendChild(el("p", { class: "hint" }, payload.telegram.reason));
    }
    if (payload.telegram && payload.telegram.sent) {
      root.appendChild(el("p", { class: "hint" }, "I messaged the owner on Telegram with Approve and Reject."));
    }
    var card = shareCard(payload.receipt);
    if (card) root.appendChild(card);
  }

  function setSteps(names, bad) {
    ["step-agent", "step-instruction", "step-wallet", "step-verdict"].forEach(function (id) {
      var node = $(id);
      if (!node) return;
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

  async function readSse(path, body, onEvent) {
    var response = await fetch(path, {
      method: "POST",
      headers: { accept: "text/event-stream", "content-type": "application/json" },
      body: JSON.stringify(body),
    });
    var type = response.headers.get("content-type") || "";
    if (type.indexOf("text/event-stream") === -1) {
      var text = await response.text();
      var parsed = null;
      try { parsed = JSON.parse(text); } catch (err) { parsed = null; }
      return { ok: response.ok, status: response.status, json: parsed, text: text };
    }
    var reader = response.body.getReader();
    var decoder = new TextDecoder();
    var buf = "";
    var payload = null;
    var failure = null;
    while (true) {
      var next = await reader.read();
      if (next.done) break;
      buf += decoder.decode(next.value, { stream: true });
      var splitAt;
      while ((splitAt = buf.indexOf("\n\n")) >= 0) {
        var chunk = buf.slice(0, splitAt);
        buf = buf.slice(splitAt + 2);
        var data = chunk.split("\n").filter(function (line) { return line.indexOf("data:") === 0; })
          .map(function (line) { return line.slice(5).trim(); }).join("");
        if (!data) continue;
        var event = null;
        try { event = JSON.parse(data); } catch (err) { event = null; }
        if (!event) continue;
        if (onEvent) onEvent(event);
        if (event.type === "result") payload = event.payload;
        if (event.type === "error") failure = event.message || "the request failed";
      }
    }
    if (failure) return { ok: false, status: 200, json: null, text: failure };
    return { ok: Boolean(payload), status: 200, json: payload, text: payload ? "" : "the stream ended early" };
  }

  function paintDrain(data) {
    if (!data) return;
    $("drain-attempts").textContent = String(data.attempts || 0);
    $("drain-refusals").textContent = String(data.refusals || 0);
    $("drain-drained").textContent = data.drainedDisplay || data.drained || "0";
    var note = clear($("drain-note"));
    note.appendChild(document.createTextNode(data.note || ""));
    var feed = clear($("drain-feed"));
    var rows = data.feed || [];
    if (!rows.length) {
      feed.appendChild(el("li", null, "No Drain Me attempts on this instance yet."));
      return;
    }
    rows.forEach(function (row) {
      var link = row.explorerUrl
        ? el("a", { href: row.explorerUrl, rel: "noopener", target: "_blank" }, "transaction")
        : document.createTextNode("no transaction");
      feed.appendChild(el("li", null,
        pill(row.outcome || "attempt", row.outcome === "settled" ? "bad" : row.outcome === "held" ? "warn" : "ok"),
        " ",
        row.prompt || "",
        row.payee ? " → " + row.payee : "",
        " ",
        link
      ));
    });
  }

  async function refreshDrain() {
    var result = await api("/api/drain");
    if (result.ok && result.json) paintDrain(result.json);
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
        "Live on NEAR testnet. Cooldown " + data.cooldownSeconds + "s. " +
        data.maxAttemptsPerDay + " signed attempts per UTC day."
      ));
    }
    paintSpend(data);
    paintRisks(data.risks);
    paintPending(data.review);
    paintRecent(data.recent);
    var telegram = clear($("telegram-note"));
    telegram.appendChild(document.createTextNode(
      data.telegram
        ? "When a payment is held, I message the owner on Telegram with Approve and Reject."
        : "Telegram is not configured. Set TELEGRAM_BOT_TOKEN and TELEGRAM_OWNER_CHAT_ID. Approve and Reject on this page still work."
    ));
    var kill = clear($("kill-note"));
    kill.appendChild(document.createTextNode(
      data.killSwitch
        ? (data.sharedLimits
          ? "Single-use is stored in the shared store, so the link dies after one open on any instance."
          : "No shared store is set. Single-use holds on this instance. A second instance could still open the same link, and the second revoke does nothing if the agent is already inactive.")
        : "Set KILL_SWITCH_SECRET on the server. Until then I cannot mint a revoke link."
    ));
    var ready = Boolean(data.signing && data.configured);
    $("btn-launch").disabled = !ready || state.busy;
    $("btn-run").disabled = !ready || state.busy;
    $("btn-drain").disabled = !ready || state.busy;
    $("btn-kill").disabled = !data.killSwitch || state.busy;
    paintReceiptFeed(data.recent);
    var policyNote = clear($("policy-note"));
    if (policyNote) {
      policyNote.appendChild(document.createTextNode(
        data.policyApply
          ? "Apply is on for this server. Preview first. Apply signs with the owner key."
          : "Apply is off on this public demo. Preview shows the diff and does not sign. Set TRIAGEPAY_POLICY_APPLY=1 to allow apply."
      ));
    }
    if (data.agent && data.explorerAccount) {
      var pass = $("passport-link");
      if (pass) pass.setAttribute("href", "/agent/" + data.agent);
    }
    loadHoneypot();
  }

  function paintReceiptFeed(rows) {
    var root = $("receipts-feed");
    if (!root) return;
    clear(root);
    var shown = (rows || []).filter(function (row) { return row.txHash; }).slice(0, 6);
    if (!shown.length) {
      root.appendChild(el("p", { class: "hint" }, "Receipts appear after a signed attempt. A replay link is added when this server stored the agent's text."));
      return;
    }
    shown.forEach(function (row) {
      var card = shareCard({
        kind: row.outcome === "success" && !row.errorCode ? "settled" : "refused",
        headline: row.errorCode || (row.outcome === "success" ? "SETTLED" : "REFUSED"),
        rule: row.errorCode || "",
        ruleText: row.title || "",
        tried: row.title || "",
        amount: row.amount || "",
        payee: row.payee || "",
        txHash: row.txHash,
        explorerUrl: row.explorerUrl,
        shareText: (row.errorCode || "RECEIPT") + " built at NEAR HACKATHON @NEARProtocol #NEARHackathon",
      });
      if (card) root.appendChild(card);
    });
  }

  async function loadHoneypot() {
    var note = $("honeypot-note");
    if (!note) return;
    var result = await api("/api/honeypot");
    if (!result.ok || !result.json) return;
    var data = result.json;
    clear(note).appendChild(document.createTextNode(data.note || ""));
    var payee = $("honeypot-payee");
    if (payee) payee.textContent = data.payee || "";
    var list = clear($("honeypot-list"));
    var rows = data.feed || [];
    if (!rows.length) {
      list.appendChild(el("li", null, "No trap payment is stored on this instance yet."));
      return;
    }
    rows.forEach(function (row) {
      var link = row.explorerUrl ? el("a", { href: row.explorerUrl, rel: "noopener", target: "_blank" }, "evidence") : document.createTextNode("no transaction");
      list.appendChild(el("li", null, (row.outcome || "refused") + (row.errorCode ? " " + row.errorCode : "") + " ", link));
    });
  }

  function paintPolicy(payload, isError) {
    var root = clear($("policy-result"));
    if (isError) {
      root.appendChild(el("p", { class: "notice bad" }, payload));
      return;
    }
    root.appendChild(el("p", null, "Parser: " + (payload.mode || "parser") + "."));
    if (payload.modelNote) root.appendChild(el("p", { class: "hint" }, payload.modelNote));
    (payload.notes || []).forEach(function (line) { root.appendChild(el("p", { class: "hint" }, line)); });
    var changes = payload.changes || [];
    if (!changes.length) root.appendChild(el("p", null, "No difference from the on-chain policy, or the sentence named nothing I can map."));
    changes.forEach(function (change) {
      root.appendChild(el("p", null, change.field + ": " + change.from + " → " + change.to));
    });
    if (payload.summary) root.appendChild(el("p", null, payload.summary));
    var apply = $("btn-policy-apply");
    if (apply) apply.disabled = !payload.applyAllowed || !changes.length || state.busy;
  }

  async function onPolicy(event, action) {
    if (event) event.preventDefault();
    var text = $("policy-text").value;
    var result = await api("/api/policy", { text: text, action: action || "preview" });
    if (!result.ok) paintPolicy(errorText(result), true);
    else paintPolicy(result.json, false);
  }

  function slam(word) {
    var stamp = $("stamp");
    stamp.textContent = word || "REFUSED";
    stamp.classList.remove("is-in");
    if (reduceMotion) return;
    void stamp.offsetWidth;
    stamp.classList.add("is-in");
  }

  async function playDiagram() {
    $("flow").classList.add("is-live");
    setSteps(["step-agent"], false);
    await sleep(280);
    setSteps(["step-agent", "step-instruction"], false);
    await sleep(280);
    setSteps(["step-agent", "step-instruction", "step-wallet"], false);
    await sleep(320);
    setSteps(["step-agent", "step-instruction", "step-wallet", "step-verdict"], true);
    slam("REFUSED");
  }

  async function onLaunch() {
    if (state.busy) return;
    state.busy = true;
    $("btn-launch").disabled = true;
    clear($("verdict"));
    $("flow").classList.add("is-live");
    $("agent-trace").textContent = "";
    setSteps(["step-agent"], false);
    var trace = "";
    try {
      var result = await readSse("/api/agent", { scenario: "injected-payee" }, function (event) {
        if (event.type === "mode") {
          $("agent-mode").textContent = event.mode === "live" ? "Agent: live" : "Agent: scripted";
        }
        if (event.type === "delta") {
          trace += event.text || "";
          $("agent-trace").textContent = trace;
          setSteps(["step-agent"], false);
        }
        if (event.type === "tool") {
          setSteps(["step-agent", "step-instruction"], false);
          if (event.tool && event.tool.receiver_id) $("inject-line").textContent = "pay " + event.tool.receiver_id;
        }
        if (event.type === "status") setSteps(["step-agent", "step-instruction", "step-wallet"], false);
      });
      var word = result.ok ? wordFor(result.json) : null;
      setSteps(["step-agent", "step-instruction", "step-wallet", "step-verdict"], !word || word.kind === "bad");
      slam(word ? word.text : "—");
      if (!result.ok) paintResult($("verdict"), result.text || errorText(result), true);
      else paintResult($("verdict"), result.json, false);
    } finally {
      state.busy = false;
      await refresh();
      await refreshDrain();
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
    if (state.busy || !state.queueScenario) return;
    state.busy = true;
    paintResult($("review-result"), "Asking the wallet to hold the payment…", false);
    try {
      var result = await api("/api/run", { scenario: state.queueScenario });
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
    for (var i = 0; i < steps.length; i++) {
      steps[i].classList.add(i === steps.length - 1 ? "is-bad" : "is-on");
      await sleep(220);
    }
    paintResult($("review-result"), "Walkthrough only. No transaction was sent. On the wallet, a held payment waits until I approve or reject it.", false);
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

  async function onDrain(event) {
    event.preventDefault();
    if (state.busy) return;
    var prompt = $("drain-prompt").value;
    state.busy = true;
    $("btn-drain").disabled = true;
    clear($("drain-result"));
    var log = clear($("drain-trace"));
    var trace = "";
    try {
      var result = await readSse("/api/drain", { prompt: prompt }, function (evt) {
        if (evt.type === "mode") trace += (trace ? "\n" : "") + (evt.label || evt.mode || "");
        if (evt.type === "delta") trace += evt.text || "";
        if (evt.type === "tool" && evt.tool) {
          trace += "\npay " + (evt.tool.receiver_id || "") + " " + (evt.tool.amount || "");
        }
        log.textContent = trace;
      });
      if (!result.ok) paintResult($("drain-result"), result.text || errorText(result), true);
      else {
        paintResult($("drain-result"), result.json, false);
        if (result.json && result.json.drain) paintDrain(result.json.drain);
      }
    } finally {
      state.busy = false;
      await refresh();
      await refreshDrain();
    }
  }

  async function onKill() {
    if (state.busy) return;
    state.busy = true;
    $("btn-kill").disabled = true;
    paintResult($("kill-result"), "Minting a one-time revoke link…", false);
    try {
      var result = await api("/api/kill", {});
      if (!result.ok) {
        paintResult($("kill-result"), errorText(result), true);
        return;
      }
      var body = result.json || {};
      var link = $("kill-link");
      link.textContent = body.url || "";
      if (body.url) link.setAttribute("href", body.url);
      if (body.qrSvg) {
        $("kill-qr").setAttribute("src", "data:image/svg+xml;charset=utf-8," + encodeURIComponent(body.qrSvg));
        $("kill-qr").setAttribute("alt", "QR code for the one-time revoke link");
      }
      paintResult($("kill-result"), "Link expires " + (body.expiresAt || "") + ". Opening it revokes the agent. Single-use source: " + (body.singleUse || "memory") + ".", false);
    } finally {
      state.busy = false;
      await refresh();
    }
  }

  $("btn-launch").addEventListener("click", onLaunch);
  $("run-form").addEventListener("submit", onRun);
  $("scenario").addEventListener("change", paintScenario);
  $("btn-refresh").addEventListener("click", function () { refresh(); refreshDrain(); });
  $("btn-revoke").addEventListener("click", function () { onOwner("revoke"); });
  $("btn-restore").addEventListener("click", function () { onOwner("restore"); });
  $("btn-queue").addEventListener("click", onQueue);
  $("btn-walk").addEventListener("click", onWalk);
  $("btn-attest").addEventListener("click", onAttest);
  $("drain-form").addEventListener("submit", onDrain);
  var policyForm = $("policy-form");
  if (policyForm) policyForm.addEventListener("submit", function (event) { onPolicy(event, "preview"); });
  var policyApply = $("btn-policy-apply");
  if (policyApply) policyApply.addEventListener("click", function () { onPolicy(null, "apply"); });
  $("btn-kill").addEventListener("click", onKill);

  refresh().then(function () { return refreshDrain(); }).then(playDiagram);
  setInterval(function () {
    if (state.busy || document.visibilityState === "hidden") return;
    refresh();
    refreshDrain();
  }, 20000);
})();
