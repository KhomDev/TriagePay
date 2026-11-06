/* Wallet policy: read-only views of a spend-policy wallet contract. */
(function () {
  "use strict";
  var A = window.APS;
  var S = window.APSSamples;
  var el = A.el;

  function $(id) { return document.getElementById(id); }

  function percent(part, whole) {
    try {
      var w = BigInt(whole);
      if (w === BigInt(0)) return 0;
      var p = Number((BigInt(part) * BigInt(10000)) / w) / 100;
      return Math.max(0, Math.min(100, p));
    } catch (err) {
      return 0;
    }
  }

  function bar(label, part, whole, decimals) {
    var pct = percent(part, whole);
    var id = "bar-" + label.replace(/\W+/g, "-").toLowerCase();
    return el("div", { class: "field" },
      el("span", { id: id }, label + ": " + A.formatAtomic(part, decimals) + " of " +
        A.formatAtomic(whole, decimals) + " (" + pct.toFixed(1) + "%)"),
      el("div", { class: "bar", role: "progressbar", "aria-labelledby": id,
        "aria-valuemin": "0", "aria-valuemax": "100", "aria-valuenow": pct.toFixed(1) },
        el("span", { style: null, "data-pct": pct.toFixed(1) })));
  }

  function nsToIso(ns) {
    try {
      return new Date(Number(BigInt(ns) / BigInt(1000000))).toISOString();
    } catch (err) {
      return String(ns);
    }
  }

  function render(data, source, network) {
    var out = A.clear($("wallet-out"));
    var decimals = Math.max(0, Math.min(24, parseInt($("w-decimals").value, 10) || 0));
    out.appendChild(el("p", null, "Data source: ", A.sourceBadge(source)));
    if (source === "mock") {
      out.appendChild(el("p", { class: "notice mock" },
        "Illustrative values for a wallet that does not exist on chain. Enter a deployed " +
        "contract id and choose a live data source to read real state."));
    }
    var c = data.config;
    out.appendChild(el("h3", null, "Wallet configuration"));
    out.appendChild(A.kv([
      ["Contract", data.contract, "mono"],
      ["Owner", c.owner, "mono"],
      ["Paused", c.paused ? "yes: every payment is refused" : "no"],
      ["Allowed tokens", (c.tokens || []).join(", ") || "none", "mono"],
      ["Next payment id", String(c.next_payment_id)],
      ["Events", c.event_standard + " " + c.event_version, "mono"]
    ]));
    if (source === "live") {
      out.appendChild(el("p", null, el("a", { href: A.explorerAccount(network, data.contract),
        rel: "noopener", target: "_blank" }, "Contract on nearblocks (new tab)")));
    }

    out.appendChild(el("h3", null, "Agent policy: ", el("span", { class: "mono" }, data.agent)));
    var a = data.agentView;
    if (!a) {
      out.appendChild(el("p", { class: "notice" }, "This wallet has no policy for that agent."));
      return;
    }
    out.appendChild(el("p", null, el("span", { class: a.active ? "pill ok" : "pill bad" },
      a.active ? "active" : "revoked")));
    out.appendChild(bar("Spent today", a.spent_in_window, a.daily_max, decimals));
    out.appendChild(bar("Largest single payment allowed vs daily cap", a.per_call_max,
      a.daily_max, decimals));
    out.appendChild(A.kv([
      ["Per-call max", A.formatAtomic(a.per_call_max, decimals)],
      ["Daily max", A.formatAtomic(a.daily_max, decimals)],
      ["Remaining today", A.formatAtomic(a.remaining, decimals)],
      ["get_spent", A.formatAtomic(data.spent, decimals)],
      ["Window resets", nsToIso(a.resets_at_ns) + " (UTC day " + a.window_index + ")"],
      ["Allowed payees", (a.payees || []).join(", ") || "none", "mono"]
    ]));
    paintBars(out);
  }

  /* Bar widths are set through the CSSOM, which the style-src 'self' CSP
   * allows (inline style attributes would not be). */
  function paintBars(root) {
    root.querySelectorAll(".bar > span").forEach(function (span) {
      span.style.width = span.getAttribute("data-pct") + "%";
    });
  }

  async function load(event) {
    if (event) event.preventDefault();
    var out = $("wallet-out");
    var network = $("w-network").value;
    var contract = $("w-contract").value.trim();
    var agent = $("w-agent").value.trim();
    var button = $("btn-wallet");
    if (A.settings().mode === "mock" || !contract) {
      var m = S.mockWallet;
      render({ contract: m.contract, agent: m.agent, config: m.config,
        agentView: m.agentView, spent: m.spent }, "mock", network);
      return;
    }
    if (!agent) {
      A.clear(out).appendChild(el("p", { class: "notice bad", role: "alert" },
        "Enter the agent account id."));
      return;
    }
    button.disabled = true;
    try {
      var results = await Promise.all([
        A.viewCall(network, contract, "get_config", {}),
        A.viewCall(network, contract, "get_agent", { agent_id: agent }),
        A.viewCall(network, contract, "get_spent", { agent_id: agent })
      ]);
      render({ contract: contract, agent: agent, config: results[0], agentView: results[1],
        spent: results[2] }, "live", network);
    } catch (err) {
      A.clear(out).appendChild(el("p", { class: "notice bad", role: "alert" },
        "Read failed: " + err.message));
    } finally {
      button.disabled = false;
    }
  }

  A.ready.then(function () {
    $("wallet-form").addEventListener("submit", load);
    load();
  });
})();
