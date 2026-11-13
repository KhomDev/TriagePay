/* Live judge demo. Text only: nothing from the chain is parsed as HTML. */
(function () {
  "use strict";

  var reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  var FLOOR_KEY = "triagepay-board-floor";
  var state = { scenarios: [], busy: false, review: null, queueScenario: null, ready: false, killSwitch: false, recentRows: null, drainRows: null, network: "testnet", mainnetSigning: false, mainnetProofs: [], mainnetRemaining: null, testnetRemaining: null, maxAttempts: 300, floor: { paid: null, refused: null, agents: null, protectedYocto: null, protectedDisplay: null, asOf: null } };

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
    var reviewOn = Boolean(state.review && state.review.reviewUnknownPayees);
    scenarios.forEach(function (scenario) {
      if (scenario.id === "review-payee" && !reviewOn) return;
      select.appendChild(el("option", { value: scenario.id }, scenario.title));
    });
    var offered = Array.prototype.map.call(select.options, function (option) { return option.value; });
    if (current && offered.indexOf(current) !== -1) select.value = current;
    else if (offered.indexOf("injected-payee") !== -1) select.value = "injected-payee";
    paintScenario();
    var injected = scenarios.filter(function (item) { return item.id === "injected-payee"; })[0];
    if (injected && injected.injection) $("inject-line").textContent = injected.injection;
  }

  function accountLink(base, id) {
    if (!base || !id) return document.createTextNode(id || "");
    return el("a", { href: base + id, rel: "noopener", target: "_blank" }, id);
  }

  function yoctoOf(value) {
    try { return BigInt(value || "0"); } catch (err) { return 0n; }
  }

  function asOfLabel(iso) {
    if (!iso) return "";
    var when = new Date(iso);
    if (isNaN(when.getTime())) return "";
    var hh = String(when.getUTCHours()).padStart(2, "0");
    var mm = String(when.getUTCMinutes()).padStart(2, "0");
    return "as of " + hh + ":" + mm + " UTC";
  }

  function loadFloor() {
    try {
      var saved = JSON.parse(sessionStorage.getItem(FLOOR_KEY) || "null");
      if (saved && typeof saved === "object") state.floor = saved;
    } catch (err) {}
  }

  function rememberFloor() {
    try { sessionStorage.setItem(FLOOR_KEY, JSON.stringify(state.floor)); } catch (err) {}
  }

  function raiseBoard(board) {
    if (!board) return;
    var floor = state.floor;
    if (board.paid != null) floor.paid = floor.paid == null ? Number(board.paid) : Math.max(floor.paid, Number(board.paid));
    if (board.refused != null) floor.refused = floor.refused == null ? Number(board.refused) : Math.max(floor.refused, Number(board.refused));
    if (board.agents != null) floor.agents = floor.agents == null ? Number(board.agents) : Math.max(floor.agents, Number(board.agents));
    if (board.protectedYocto != null && (floor.protectedYocto == null || yoctoOf(board.protectedYocto) >= yoctoOf(floor.protectedYocto))) {
      floor.protectedYocto = String(board.protectedYocto);
      if (board.protectedDisplay) floor.protectedDisplay = board.protectedDisplay;
    }
    if (board.asOf && (!floor.asOf || String(board.asOf) > String(floor.asOf))) floor.asOf = board.asOf;
    rememberFloor();
    paintNumbers(floor, board);
  }

  function paintNumbers(floor, board) {
    if (!$("num-paid")) return;
    var shown = floor || board;
    if (!shown || shown.paid == null) return;
    $("num-paid").textContent = String(shown.paid);
    $("num-refused").textContent = shown.refused == null ? "—" : String(shown.refused);
    $("num-protected").textContent = shown.protectedDisplay || (board && board.protectedDisplay) || "—";
    $("num-agents").textContent = shown.agents == null ? "—" : String(shown.agents);
    var source = $("num-source");
    if (source) {
      clear(source);
      source.appendChild(el("a", { href: (board && board.sourceUrl) || "https://nearblocks.io", rel: "noopener", target: "_blank" }, "Live from NearBlocks"));
      var stamp = asOfLabel(shown.asOf || (board && board.asOf));
      if (stamp) source.appendChild(document.createTextNode(" · " + stamp));
    }
  }

  function paintAttemptLeft() {
    var left = $("attempt-left");
    if (!left) return;
    if (state.testnetRemaining == null) return;
    left.classList.remove("is-skeleton");
    left.removeAttribute("aria-busy");
    clear(left);
    left.appendChild(document.createTextNode(state.testnetRemaining + " testnet attempts left today"));
  }

  function noteRemaining(value) {
    if (value == null || !isFinite(Number(value))) return;
    var next = Number(value);
    if (state.testnetRemaining == null || next < state.testnetRemaining) state.testnetRemaining = next;
    paintAttemptLeft();
  }

  function loadStaticBoard() {
    return fetch("/scoreboard.json", { cache: "no-store" }).then(function (res) {
      if (!res.ok) return null;
      return res.json();
    }).then(function (board) {
      if (!board) return;
      raiseBoard(board);
      noteRemaining(board.attemptsRemaining);
    }).catch(function () {});
  }

  function paintMainnetProofs() {
    var box = $("mainnet-proofs");
    if (!box) return;
    var show = state.network === "mainnet";
    box.hidden = !show;
    if (!show) return;
    clear(box);
    box.appendChild(document.createTextNode("Mainnet proofs: "));
    (state.mainnetProofs || []).forEach(function (item, index) {
      if (index) box.appendChild(document.createTextNode(" · "));
      box.appendChild(el("a", { href: item.href, rel: "noopener", target: "_blank" }, item.label));
    });
  }

  function setNetwork(name) {
    state.network = name === "mainnet" ? "mainnet" : "testnet";
    var testnet = $("net-testnet");
    var mainnet = $("net-mainnet");
    if (testnet) testnet.setAttribute("aria-pressed", state.network === "testnet" ? "true" : "false");
    if (mainnet) mainnet.setAttribute("aria-pressed", state.network === "mainnet" ? "true" : "false");
    paintMainnetProofs();
    var left = $("mainnet-left");
    if (left) {
      var showLeft = state.network === "mainnet" && state.mainnetRemaining != null;
      left.hidden = !showLeft;
      if (showLeft) left.textContent = state.mainnetRemaining + " mainnet runs left today";
    }
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
    if (!state.holdStatus) $("agent-mode").textContent = "Deterministic attack, real on-chain refusal";
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
    var threshold = review.approvalThresholdDisplay || review.approvalThreshold || "0";
    copy.appendChild(document.createTextNode(
      "An attacker payee is refused, not held. A payment above " + threshold + " is held. Queue one, then Approve it. " +
      (review.pendingCount || 0) + " waiting."
    ));
    state.queueScenario = BigInt(review.approvalThreshold || "0") > 0n ? "above-threshold" : null;
    $("btn-queue").textContent = "Queue a payment above the threshold";
    $("btn-queue").disabled = !state.queueScenario || state.busy;
    var rows = review.pending || [];
    if (!rows.length) {
      root.appendChild(el("p", { class: "hint" }, "Nothing is waiting. Queue a payment above the threshold, then approve or reject it here."));
      return;
    }
    rows.forEach(function (row) {
      var id = String(row.payment_id);
      root.appendChild(el("p", null,
        pill(row.reason || "held", "warn"),
        " payment #",
        id,
        " ",
        el("span", { class: "mono" }, row.receiver_id || ""),
        " ",
        row.amountDisplay || ""
      ));
      var actions = el("div", { class: "actions" });
      var approve = el("button", { type: "button" }, "Approve payment #" + id);
      var reject = el("button", { type: "button", class: "secondary" }, "Reject payment #" + id);
      approve.addEventListener("click", function () { onReview("approve", id); });
      reject.addEventListener("click", function () { onReview("reject", id); });
      actions.appendChild(approve);
      actions.appendChild(reject);
      root.appendChild(actions);
    });
  }

  function paintRecent(rows) {
    if ((!rows || !rows.length) && state.recentRows && state.recentRows.length) return;
    state.recentRows = rows || [];
    var body = clear($("recent"));
    if (!rows || !rows.length) {
      body.appendChild(el("tr", null, el("td", { colspan: "5" }, "No signed attempts yet. Launch the attack and the transaction shows up here.")));
      return;
    }
    rows.forEach(function (row) {
      var when = row.atMs ? new Date(row.atMs).toISOString().replace("T", " ").slice(0, 19) + " UTC" : "";
      var chain = row.outcome === "success" ? "settled" : (row.outcome === "failure" ? "refused" : (row.errorCode || row.outcome || "refused"));
      var risk = row.riskVerdict || (row.errorCode ? "block" : (row.outcome === "success" ? "allow" : "block"));
      var link = row.explorerUrl
        ? el("a", { href: row.explorerUrl, rel: "noopener", target: "_blank" }, "view")
        : document.createTextNode("");
      body.appendChild(el("tr", null,
        el("td", null, when),
        el("td", null, [row.title || row.kind || "", row.payee || "", withNear(row.amount || "")].filter(Boolean).join(" ")),
        el("td", null, risk ? pill(risk, risk) : document.createTextNode("")),
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
    var rows = el("dl", null);
    function addRow(label, value, mono) {
      if (!value) return;
      rows.appendChild(el("dt", null, label));
      rows.appendChild(el("dd", mono ? { class: "mono" } : null, value));
    }
    addRow("Rule", receipt.rule ? (receipt.rule + (receipt.ruleText ? " — " + receipt.ruleText : "")) : "");
    addRow("Amount", receipt.amount);
    addRow("Payee", receipt.payee, true);
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
    if (payload.agent) {
      root.appendChild(el("p", null, agentLabel(payload.agent)));
    }
    if (payload.summary) root.appendChild(el("p", null, payload.summary));
    if (payload.risk) root.appendChild(el("p", null, "Payee risk ", pill(payload.risk.verdict, payload.risk.verdict)));
    if (payload.amountCheck) root.appendChild(el("p", null, "Amount check ", pill(payload.amountCheck.verdict, payload.amountCheck.verdict)));
    var chain = payload.chain;
    if (chain && chain.errorCode) {
      root.appendChild(el("p", null, "Contract ", el("span", { class: "mono" }, chain.errorCode)));
    }
    if (chain && chain.explorerUrl) {
      root.appendChild(el("p", null, el("a", { href: chain.explorerUrl, rel: "noopener", target: "_blank" }, "Open the transaction")));
    }
    if (payload.paid && payload.paid.explorerUrl) {
      root.appendChild(el("p", null, "Allowed call ", el("a", { href: payload.paid.explorerUrl, rel: "noopener", target: "_blank" }, "Open the paid transaction")));
    }
    if (payload.telegram && payload.telegram.sent) {
      root.appendChild(el("p", { class: "hint" }, "I messaged the owner with Approve and Reject."));
    }
    var card = shareCard(payload.receipt);
    if (card) root.appendChild(card);
  }

  function withNear(text) {
    var value = String(text || "");
    if (!value || /[A-Za-z]/.test(value)) return value;
    var over = /^(\d+\.\d{0,6}?)0{8,}1$/.exec(value);
    if (over) return (over[1].replace(/\.$/, "") || "0") + " wNEAR plus 1 yocto";
    return value + " wNEAR";
  }

  function agentLabel(agent) {
    if (agent && agent.mode === "live") return "Live model";
    return "Deterministic attack, real on-chain refusal";
  }

  function syncControls() {
    var busy = Boolean(state.busy);
    var ready = Boolean(state.ready);
    var blocked = (state.network === "mainnet" && state.mainnetRemaining === 0)
      || (state.network !== "mainnet" && state.testnetRemaining === 0);
    if ($("btn-launch")) $("btn-launch").disabled = !ready || busy || blocked;
    if ($("btn-run")) $("btn-run").disabled = !ready || busy;
    if ($("btn-drain")) $("btn-drain").disabled = !ready || busy;
    if ($("btn-kill")) $("btn-kill").disabled = !state.killSwitch || busy;
    if ($("btn-attest")) $("btn-attest").disabled = busy;
    if ($("btn-queue")) $("btn-queue").disabled = !state.queueScenario || busy;
  }

  function showLiveVerdict(word, payload) {
    var box = clear($("live-verdict"));
    if (!box) return;
    if (!word) return;
    box.appendChild(el("p", { class: "verdict-word " + word.kind }, word.text));
    var chain = payload && payload.chain;
    if (chain && chain.errorCode) box.appendChild(el("p", null, el("span", { class: "mono" }, chain.errorCode)));
    if (chain && chain.explorerUrl) {
      box.appendChild(el("p", null, el("a", { href: chain.explorerUrl, rel: "noopener", target: "_blank" }, "Open the transaction")));
    }
    if (window.matchMedia("(max-width: 900px)").matches) {
      box.scrollIntoView({ block: "nearest", behavior: reduceMotion ? "auto" : "smooth" });
    }
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

  async function apiRead(path) {
    var result = await api(path);
    if (result.ok || (result.status && result.status < 500)) return result;
    await sleep(400);
    return api(path);
  }

  function errorText(result) {
    if (result.json && result.json.error && result.json.error.message) return result.json.error.message;
    var text = String(result.text || "");
    if (/<!doctype|<html/i.test(text)) return "The server sent a page instead of data (" + (result.status || "error") + "). Refresh and try again.";
    if (text.length > 220) return text.slice(0, 220);
    return text || ("request failed (" + result.status + ")");
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
    var incoming = data.feed || [];
    if (!incoming.length && state.drainRows && state.drainRows.length) return;
    state.drainRows = incoming;
    $("drain-attempts").textContent = String(data.attempts || 0);
    $("drain-refusals").textContent = String(data.refusals || 0);
    $("drain-drained").textContent = data.drainedDisplay || data.drained || "0";
    var note = clear($("drain-note"));
    note.appendChild(document.createTextNode(data.note || ""));
    var feed = clear($("drain-feed"));
    var rows = data.feed || [];
    if (!rows.length) {
      feed.appendChild(el("li", null, "No one has tried yet. Write a prompt above and press Try to drain it."));
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
    if (!state.holdStatus) {
      clear(status);
      status.appendChild(document.createTextNode("Reading the wallet on testnet…"));
    }
    var result = await apiRead("/api/dashboard");
    if (state.holdStatus) {
      // The launch verdict stays. Stats still update below.
    } else {
      clear(status);
    }
    if (!result.ok || !result.json) {
      if (state.ready) {
        status.appendChild(document.createTextNode("The wallet read failed. The last numbers are still on the page."));
        return;
      }
      status.appendChild(el("span", { class: "notice bad" }, errorText(result)));
      return;
    }
    var data = result.json;
    state.scenarios = data.scenarios || [];
    state.review = data.review || null;
    fillSelect(state.scenarios);
    if (!data.configured) {
      status.appendChild(document.createTextNode(data.message || "Demo wallet is not configured."));
    } else if (!state.holdStatus && !data.signing) {
      status.appendChild(document.createTextNode("I can read the chain. Signing is off, so Launch the attack stays off."));
    } else if (!state.holdStatus) {
      var wait = Number(data.cooldownRemainingSeconds || 0);
      status.appendChild(document.createTextNode(
        "Live on NEAR testnet. " +
        (wait > 0 ? ("Next signature in " + wait + "s. ") : ("Cooldown " + data.cooldownSeconds + "s. ")) +
        data.maxAttemptsPerDay + " signed attempts per UTC day."
      ));
    }
    state.mainnetSigning = Boolean(data.mainnet && data.mainnet.signing);
    state.mainnetProofs = (data.mainnet && data.mainnet.proofs) || [];
    state.mainnetRemaining = data.mainnet && data.mainnet.remaining != null ? data.mainnet.remaining : null;
    state.maxAttempts = data.maxAttemptsPerDay || state.maxAttempts;
    noteRemaining(data.attemptsRemaining);
    raiseBoard(data.scoreboard);
    paintMainnetProofs();
    paintSpend(data);
    paintRisks(data.risks);
    paintPending(data.review);
    paintRecent(data.recent);
    var telegram = clear($("telegram-note"));
    telegram.appendChild(document.createTextNode(
      data.telegram
        ? "When a payment is held, I message the owner with Approve and Reject."
        : "Owner messages are off. Approve and Reject on this page still work."
    ));
    var kill = clear($("kill-note"));
    kill.appendChild(document.createTextNode(
      data.killSwitch
        ? "The link works once, then it dies."
        : "The one-time link is off on this server."
    ));
    state.ready = Boolean(data.signing && data.configured);
    state.killSwitch = Boolean(data.killSwitch);
    syncControls();
    paintReceiptFeed(data.recent);
    var policyNote = clear($("policy-note"));
    if (policyNote) {
      policyNote.appendChild(document.createTextNode(
        data.policyApply
          ? "Apply is on. Preview first. Apply signs with the owner key."
          : "Preview shows the difference and does not sign."
      ));
    }
    if (data.agent && data.explorerAccount) {
      var pass = $("passport-link");
      if (pass) pass.setAttribute("href", "/agent/" + data.agent);
    }
    loadHoneypot();
  }

  function ruleSentence(row) {
    if (row.outcome === "held") return "The wallet held the payment for the owner.";
    if (row.outcome === "success" && !row.errorCode) return "The wallet settled the payment.";
    if (row.errorCode === "ERR_PAYEE_NOT_ALLOWED") return "The payee is not on the allowlist.";
    if (row.errorCode === "ERR_PER_CALL_LIMIT") return "The amount is above the per-call cap.";
    if (row.errorCode === "ERR_DAILY_LIMIT") return "The amount would pass the daily cap.";
    if (row.errorCode) return "The wallet refused the payment.";
    if (row.title && row.title !== "pay") return row.title;
    return "The wallet refused the payment.";
  }

  function paintReceiptFeed(rows) {
    var root = $("receipts-feed");
    if (!root) return;
    var shown = (rows || []).filter(function (row) { return row.txHash && (row.payee || row.amount || row.errorCode || row.title); });
    if (!shown.length && state.receiptRows && state.receiptRows.length) return;
    state.receiptRows = shown;
    clear(root);
    if (!shown.length) {
      root.appendChild(el("p", { class: "hint" }, "No receipt yet. Launch the attack. The error code and the transaction link show up here."));
      return;
    }
    var featured = shown[0];
    var rest = shown.slice(1, 6);
    [featured].forEach(function (row) {
      var card = shareCard({
        kind: row.outcome === "held" ? "held" : (row.outcome === "success" && !row.errorCode ? "settled" : "refused"),
        headline: row.outcome === "held" ? "HELD" : (row.outcome === "success" && !row.errorCode ? "SETTLED" : "REFUSED"),
        rule: row.errorCode || "",
        ruleText: ruleSentence(row),
        tried: ruleSentence(row),
        amount: withNear(row.amount || ""),
        payee: row.payee || "",
        txHash: row.txHash,
        explorerUrl: row.explorerUrl,
        shareText: (row.outcome === "held" ? "HELD" : (row.outcome === "success" && !row.errorCode ? "SETTLED" : "REFUSED")) + ". " + ruleSentence(row) + " built at NEAR HACKATHON @NEARProtocol #NEARHackathon",
      });
      if (card) root.appendChild(card);
    });
    if (rest.length) {
      var list = el("ol", { class: "feed" });
      rest.forEach(function (row) {
        var link = row.explorerUrl
          ? el("a", { href: row.explorerUrl, rel: "noopener", target: "_blank" }, "transaction")
          : document.createTextNode("no transaction");
        list.appendChild(el("li", null, (row.errorCode || row.outcome || "receipt") + " ", row.payee || "", " ", row.amount || "", " ", link));
      });
      root.appendChild(list);
    }
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
    var rows = data.feed || [];
    var panel = document.querySelector(".honeypot");
    if (!rows.length && state.honeypotRows && state.honeypotRows.length) return;
    if (panel) panel.hidden = !rows.length;
    state.honeypotRows = rows;
    var list = clear($("honeypot-list"));
    if (!rows.length) {
      list.appendChild(el("li", null, "No refusal to this trap is on the chain index yet. Launch the attack to add one."));
      return;
    }
    rows.forEach(function (row) {
      var when = row.atMs ? new Date(row.atMs).toISOString().replace("T", " ").slice(0, 19) + " UTC" : "";
      var link = row.explorerUrl ? el("a", { href: row.explorerUrl, rel: "noopener", target: "_blank" }, "transaction") : document.createTextNode("no transaction");
      list.appendChild(el("li", null,
        row.payee || "payee",
        " ",
        row.amount ? withNear(row.amount) + " " : "",
        when ? when + " " : "",
        (row.errorCode ? row.errorCode + " " : ""),
        link
      ));
    });
  }

  function paintPolicy(payload, isError) {
    var root = clear($("policy-result"));
    if (isError) {
      root.appendChild(el("p", { class: "notice bad" }, payload));
      return;
    }
    root.appendChild(el("p", null, payload.mode === "live"
      ? "A model agreed with this reading."
      : "I read the sentence and compared it with the policy on the wallet."));
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
    if (action === "apply") {
      var applied = await api("/api/policy", { text: text, action: "apply" });
      if (!applied.ok) paintPolicy(errorText(applied), true);
      else paintPolicy(applied.json, false);
      return;
    }
    var fast = await api("/api/policy", { text: text, action: "preview", fast: true });
    if (!fast.ok) paintPolicy(errorText(fast), true);
    else paintPolicy(fast.json, false);
    var result = await api("/api/policy", { text: text, action: "preview" });
    if (result.ok) paintPolicy(result.json, false);
  }

  function slam(word) {
    var wait = $("verdict-wait");
    if (wait) wait.hidden = true;
    var stamp = $("stamp");
    if (!stamp) return;
    stamp.hidden = false;
    stamp.textContent = word || "";
    stamp.classList.remove("is-in");
    if (reduceMotion || !word) return;
    void stamp.offsetWidth;
    stamp.classList.add("is-in");
  }

  function setStatus(text) {
    var status = $("status");
    clear(status);
    status.appendChild(document.createTextNode(text));
  }

  async function onLaunch() {
    if (state.busy) return;
    if (state.network !== "mainnet" && state.testnetRemaining === 0) {
      setStatus("This demo has used its " + state.maxAttempts + " signed attempts for the UTC day.");
      return;
    }
    if (state.network === "mainnet" && state.mainnetRemaining === 0) {
      paintMainnetProofs();
      setStatus("No mainnet runs left today. The proof links stay on the page.");
      return;
    }
    if (state.network === "mainnet" && !state.mainnetSigning) {
      paintMainnetProofs();
      setStatus("Mainnet signing is off. The proof links stay on the page.");
      return;
    }
    state.busy = true;
    state.holdStatus = true;
    $("btn-launch").disabled = true;
    $("btn-launch").classList.add("is-busy");
    clear($("verdict"));
    clear($("live-verdict"));
    $("flow").classList.add("is-live");
    $("agent-trace").textContent = "Reading the finding.";
    var waiting = $("verdict-wait");
    if (waiting) waiting.hidden = false;
    var stamp = $("stamp");
    if (stamp) {
      stamp.hidden = true;
      stamp.textContent = "";
    }
    $("sign-line").textContent = "Waiting to sign.";
    setStatus(state.network === "mainnet" ? "Signing on NEAR mainnet…" : "Signing the deterministic attack…");
    setSteps(["step-agent"], false);
    try {
      var launchBody = { scenario: "injected-payee" };
      if (state.network === "mainnet") launchBody.network = "mainnet";
      if ($("opt-live") && $("opt-live").checked && state.network !== "mainnet") launchBody.live = true;
      var result = await readSse("/api/agent", launchBody, function (event) {
        if (event.type === "mode") {
          $("agent-mode").textContent = event.mode === "live" ? "Live model" : "Deterministic attack, real on-chain refusal";
        }
        if (event.type === "tool") {
          setSteps(["step-agent", "step-instruction"], false);
          if (event.tool && event.tool.receiver_id) $("inject-line").textContent = "Pay " + event.tool.receiver_id;
          $("agent-trace").textContent = "The agent named a payee.";
          setStatus("Injection found. Signing pay…");
        }
        if (event.type === "status") {
          setSteps(["step-agent", "step-instruction", "step-wallet"], false);
          var netName = state.network === "mainnet" ? "mainnet" : "testnet";
          $("sign-line").textContent = "Signing pay on " + netName + ".";
          setStatus(event.text || ("Signing pay on NEAR " + netName + "."));
        }
      });
      var word = result.ok ? wordFor(result.json) : null;
      var chain = result.ok && result.json ? result.json.chain : null;
      var code = chain && chain.errorCode ? chain.errorCode : "";
      var link = chain && chain.explorerUrl ? chain.explorerUrl : "";
      setSteps(["step-agent", "step-instruction", "step-wallet", "step-verdict"], !word || word.kind === "bad");
      if (word) slam(word.text);
      if (result.ok && result.json && result.json.agent) {
        $("agent-mode").textContent = agentLabel(result.json.agent);
      }
      if (!result.ok) {
        setStatus("This attempt did not finish. There is no transaction link.");
        paintResult($("verdict"), result.text || errorText(result), true);
      } else if (!link) {
        setStatus((word ? word.text : "This attempt did not reach the chain") + ". There is no transaction link.");
        paintResult($("verdict"), result.json, false);
      } else {
        if (state.network === "mainnet" && state.mainnetRemaining != null) {
          state.mainnetRemaining = Math.max(0, state.mainnetRemaining - 1);
          var remain = $("mainnet-left");
          if (remain) remain.textContent = state.mainnetRemaining + " mainnet runs left today";
        }
        if (state.testnetRemaining != null) {
          state.testnetRemaining = Math.max(0, state.testnetRemaining - 1);
          var attemptLeft = $("attempt-left");
          if (attemptLeft) attemptLeft.textContent = state.testnetRemaining + " testnet attempts left today";
        }
        setStatus((word ? word.text : "Done") + (code ? " " + code : "") + ". The transaction link is next to the button.");
        showLiveVerdict(word, result.json);
        paintResult($("verdict"), result.json, false);
      }
    } finally {
      var keptMainnet = state.mainnetRemaining;
      var keptTestnet = state.testnetRemaining;
      $("btn-launch").classList.remove("is-busy");
      state.busy = false;
      await refresh();
      if (keptMainnet != null && state.mainnetRemaining != null && state.mainnetRemaining > keptMainnet) {
        state.mainnetRemaining = keptMainnet;
        var remain = $("mainnet-left");
        if (remain && state.network === "mainnet") remain.textContent = keptMainnet + " mainnet runs left today";
      }
      if (keptTestnet != null && state.testnetRemaining != null && state.testnetRemaining > keptTestnet) {
        state.testnetRemaining = keptTestnet;
        var attemptLeft = $("attempt-left");
        if (attemptLeft) attemptLeft.textContent = keptTestnet + " testnet attempts left today";
      }
      syncControls();
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
    var sure = window.confirm(action === "revoke" ? "Revoke the agent on testnet?" : "Restore the agent on testnet?");
    if (!sure) return;
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
      steps[i].classList.add("is-on");
      await sleep(220);
    }
    paintResult($("review-result"), "Walkthrough only. No transaction was sent. Queue a payment above the threshold, then Approve it. That hold is the owner-review path.", false);
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
      syncControls();
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
        if (evt.type === "mode") trace = evt.mode === "live" ? "Live. " : "Scripted. ";
        if (evt.type === "delta") {
          var piece = evt.text || "";
          if (piece.indexOf("Call pay with the NEAR account") === -1) trace += piece;
        }
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
    if (!window.confirm("Create a one-time link that revokes the agent?")) return;
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
      paintResult($("kill-result"), "The link works once and expires " + (body.expiresAt || "") + ". Opening it revokes the agent.", false);
    } finally {
      state.busy = false;
      await refresh();
    }
  }

  $("btn-launch").addEventListener("click", onLaunch);
  var netTestnet = $("net-testnet");
  var netMainnet = $("net-mainnet");
  if (netTestnet) netTestnet.addEventListener("click", function () { setNetwork("testnet"); });
  if (netMainnet) netMainnet.addEventListener("click", function () { setNetwork("mainnet"); });
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

  loadFloor();
  paintAttemptLeft();
  if (state.floor.paid != null) paintNumbers(state.floor, state.floor);
  loadStaticBoard();
  refresh().then(function () { return refreshDrain(); });
  setInterval(function () {
    if (state.busy || document.visibilityState === "hidden") return;
    refresh();
    refreshDrain();
  }, 20000);
})();
