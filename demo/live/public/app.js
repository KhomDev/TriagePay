/* Live judge demo. Text only: nothing from the chain is parsed as HTML. */
(function () {
  "use strict";

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

  var state = { scenarios: [], busy: false };

  function selected() {
    var id = $("scenario").value;
    return state.scenarios.filter(function (item) { return item.id === id; })[0] || null;
  }

  function paintScenario() {
    var scenario = selected();
    var hint = $("scenario-hint");
    var injection = $("injection");
    var button = $("btn-run");
    clear(hint);
    if (!scenario) {
      injection.value = "";
      button.textContent = "Run attack";
      return;
    }
    hint.appendChild(document.createTextNode(scenario.narrative || ""));
    injection.value = scenario.injection || "No injected instruction. This payment stays inside the policy.";
    button.textContent = scenario.kind === "attack" ? "Run attack" : "Run allowed payment";
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
    var label = $("spend-label");
    clear(label);
    if (!spend) {
      label.appendChild(document.createTextNode(data.message || "Wallet views are not available."));
      return;
    }
    label.appendChild(document.createTextNode(
      "Spent today " + spend.spentDisplay + " of " + spend.dailyDisplay +
      " (" + spend.percent.toFixed(1) + "%). Remaining " + spend.remainingDisplay + "."
    ));
    $("spend-bar").setAttribute("aria-valuenow", spend.percent.toFixed(1));
    $("spend-bar-fill").style.width = spend.percent.toFixed(1) + "%";
    var meta = clear($("spend-meta"));
    meta.appendChild(document.createTextNode(
      (spend.active ? "Agent is active. " : "Agent is revoked. ") +
      (data.paused ? "Wallet is paused. " : "") +
      "Per-call cap " + spend.perCallDisplay + ". " +
      (spend.resetsAt ? "Window resets " + spend.resetsAt + ". " : "")
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
        risk.ageSource ? " (age: " + risk.ageSource + ", code from RPC)" : " (code from RPC)"
      ));
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
    if (payload.summary) root.appendChild(el("p", null, payload.summary));
    if (payload.risk) {
      root.appendChild(el("p", null, "Payee risk ", pill(payload.risk.verdict, payload.risk.verdict)));
      (payload.risk.reasons || []).forEach(function (reason) {
        root.appendChild(el("p", { class: "hint" }, reason.text));
      });
    }
    if (payload.amountCheck) {
      root.appendChild(el("p", null, "Amount check ", pill(payload.amountCheck.verdict, payload.amountCheck.verdict)));
      (payload.amountCheck.reasons || []).forEach(function (reason) {
        root.appendChild(el("p", { class: "hint" }, reason.text));
      });
    }
    var chain = payload.chain;
    if (chain && chain.submitted) {
      var label = chain.outcome === "success" ? "settled" : (chain.errorCode || chain.outcome || "refused");
      root.appendChild(el("p", null, "Chain ", pill(label, chain.outcome === "success" ? "success" : "bad")));
      if (chain.detail && chain.errorCode == null) {
        root.appendChild(el("p", { class: "hint" }, chain.detail));
      }
      if (chain.explorerUrl) {
        root.appendChild(el("p", null, el("a", { href: chain.explorerUrl, rel: "noopener", target: "_blank" }, "Transaction on the testnet explorer")));
      }
    }
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
    fillSelect(state.scenarios);
    if (!data.configured) {
      status.appendChild(document.createTextNode(data.message || "Demo wallet is not configured."));
    } else if (!data.signing) {
      status.appendChild(document.createTextNode("Chain reads are live. Signing keys are not set on this server, so Run and Revoke stay off."));
    } else {
      status.appendChild(document.createTextNode(
        "Live on NEAR testnet. Cooldown " + data.cooldownSeconds + "s, at most " +
        data.maxAttemptsPerDay + " signed attempts per UTC day."
      ));
    }
    paintSpend(data);
    paintRisks(data.risks);
    paintRecent(data.recent);
    $("btn-run").disabled = !data.signing || !data.configured;
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

  $("run-form").addEventListener("submit", onRun);
  $("scenario").addEventListener("change", paintScenario);
  $("btn-refresh").addEventListener("click", function () { refresh(); });
  $("btn-revoke").addEventListener("click", function () { onOwner("revoke"); });
  $("btn-restore").addEventListener("click", function () { onOwner("restore"); });
  refresh();
})();
