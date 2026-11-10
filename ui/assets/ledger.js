/* Ledger: an account's balances and conservation, a job and its receipts. */
(function () {
  "use strict";
  var A = window.APS;
  var S = window.APSSamples;
  var el = A.el;

  function $(id) { return document.getElementById(id); }

  function conserved(b) {
    try {
      return BigInt(b.deposited_total.atomic) ===
        BigInt(b.available.atomic) + BigInt(b.reserved.atomic) + BigInt(b.settled_total.atomic);
    } catch (err) {
      return false;
    }
  }

  function balancesTable(b) {
    var rows = [["Available", b.available], ["Reserved", b.reserved],
      ["Settled (lifetime)", b.settled_total], ["Refunded (lifetime)", b.refunded_total],
      ["Deposited (lifetime)", b.deposited_total]];
    return el("div", { class: "table-wrap" }, el("table", null,
      el("caption", { class: "hint" }, "Balances in USDC; atomic units are exact"),
      el("thead", null, el("tr", null, el("th", { scope: "col" }, "Bucket"),
        el("th", { scope: "col" }, "USDC"), el("th", { scope: "col" }, "Atomic units"))),
      el("tbody", null, rows.map(function (r) {
        return el("tr", null, el("th", { scope: "row" }, r[0]), el("td", null, r[1].decimal),
          el("td", { class: "mono" }, r[1].atomic));
      }))));
  }

  function renderAccount(account, balances, source) {
    var out = A.clear($("account-out"));
    out.appendChild(el("h3", null, "Account ", el("span", { class: "mono" }, account), " ",
      A.sourceBadge(source)));
    if (source === "mock") {
      out.appendChild(el("p", { class: "notice mock" }, "Illustrative balances."));
    }
    out.appendChild(balancesTable(balances));
    var ok = conserved(balances);
    out.appendChild(el("p", null, "Conservation (deposited = available + reserved + settled): ",
      el("span", { class: ok ? "pill ok" : "pill bad" }, ok ? "holds" : "VIOLATED")));
  }

  async function loadAccount(event) {
    event.preventDefault();
    var account = $("l-account").value.trim();
    var out = $("account-out");
    if (!account) {
      A.clear(out).appendChild(el("p", { class: "notice bad", role: "alert" },
        "Enter an account id."));
      return;
    }
    if (A.mode() === "mock") {
      renderAccount(account, S.mockLedger.balances, "mock");
      return;
    }
    var res = await A.api("GET", "/accounts/" + encodeURIComponent(account));
    if (!res.ok) {
      A.clear(out).appendChild(el("p", { class: "notice bad", role: "alert" }, A.apiError(res)));
      return;
    }
    renderAccount(account, res.json, "live");
  }

  function receipt(title, r, extra) {
    if (!r) return null;
    return [el("h3", null, title), A.kv([["Job", r.job_id, "mono"]].concat(extra))];
  }

  async function loadJob(event) {
    event.preventDefault();
    var jobId = $("l-job").value.trim();
    var out = A.clear($("job-out"));
    if (!jobId) {
      out.appendChild(el("p", { class: "notice bad", role: "alert" }, "Enter a job id."));
      return;
    }
    var job;
    var source = A.mode();
    if (source === "mock") {
      job = Object.assign({}, S.mockLedger.jobs[0], {
        settlement: S.mockSettlement(S.recordedReports["vault-rounding"].content_hash)
      });
    } else {
      var res = await A.api("GET", "/jobs/" + encodeURIComponent(jobId));
      if (!res.ok) {
        out.appendChild(el("p", { class: "notice bad", role: "alert" }, A.apiError(res)));
        return;
      }
      job = res.json;
    }
    out.appendChild(el("h3", null, "Job ", el("span", { class: "mono" }, job.job_id), " ",
      A.sourceBadge(source)));
    out.appendChild(A.kv([["Account", job.account, "mono"], ["SKU", job.sku, "mono"],
      ["Amount", job.amount.decimal + " USDC"], ["State", job.state]]));
    (receipt("Settlement receipt", job.settlement, job.settlement ? [
      ["Charged", job.settlement.charged.decimal + " USDC"],
      ["Report hash", job.settlement.report_hash_hex, "mono hash"]] : []) || [])
      .forEach(function (n) { out.appendChild(n); });
    (receipt("Refund receipt", job.refund, job.refund ? [
      ["Refunded", job.refund.refunded.decimal + " USDC"],
      ["Reason", job.refund.reason]] : []) || [])
      .forEach(function (n) { out.appendChild(n); });
  }

  function renderGlobal() {
    var out = A.clear($("global-out"));
    out.appendChild(el("p", null,
      "The ledger core keeps every account conserved on each operation. The durable journal " +
      "(aps-store) re-checks conservation when it opens and refuses to start on a violation; " +
      "wiring it into this API is planned, as is a public route for the global result (",
      el("code", null, "GET /ledger/status"), ")."));
    var last = A.unstash("aps-last-scan");
    if (last && last.settlement) {
      out.appendChild(el("p", null, "Last job from Try it in this tab: ",
        el("span", { class: "mono" }, last.settlement.job_id), " ",
        A.sourceBadge(last.source)));
    }
  }

  A.ready.then(function () {
    $("account-form").addEventListener("submit", loadAccount);
    $("job-form").addEventListener("submit", loadJob);
    renderGlobal();
  });
})();
