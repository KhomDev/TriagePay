/* Try it: demo credit, a paid scan.v1 run, and the verdict. */
(function () {
  "use strict";
  var A = window.APS;
  var S = window.APSSamples;
  var V = window.APSVerdict;
  var el = A.el;

  var DECISION_CLASS = { supported: "ok", needs_evidence: "warn", held: "warn", rejected: "bad" };

  function $(id) { return document.getElementById(id); }

  function busy(button, on) {
    button.disabled = on;
    button.setAttribute("aria-busy", on ? "true" : "false");
  }

  function fail(out, message) {
    A.clear(out).appendChild(el("p", { class: "notice bad", role: "alert" }, message));
  }

  function loadSample() {
    $("envelope").value = JSON.stringify(S.envelopes[$("sample").value].envelope, null, 2);
  }

  async function credit() {
    var out = $("credit-out");
    var account = $("account").value.trim();
    var button = $("btn-credit");
    if (!account) { fail(out, "Enter an account id first."); return; }
    busy(button, true);
    try {
      if (A.mode() === "mock") {
        var mock = JSON.parse(JSON.stringify(S.mockCredit));
        mock.deposit.account = account;
        renderCredit(mock, "mock");
        return;
      }
      var intent = await A.api("POST", "/payment/intents", {
        account_id: account,
        asset_id: "asset.usdc.test",
        amount_atomic: "2000000",
        expires_in_secs: 3600
      });
      if (!intent.ok) throw new Error(A.apiError(intent));
      var observed = await A.api("POST", "/payment/intents/" +
        encodeURIComponent(intent.json.intent_id) + "/observations");
      if (!observed.ok) throw new Error(A.apiError(observed));
      renderCredit(Object.assign({ intent_id: intent.json.intent_id }, observed.json), "live");
    } catch (err) {
      fail(out, "Demo credit failed: " + err.message);
    } finally {
      busy(button, false);
    }
  }

  function renderCredit(body, source) {
    var out = A.clear($("credit-out"));
    out.appendChild(el("h3", null, "Demo credit ", A.sourceBadge(source)));
    out.appendChild(el("p", { class: "notice" },
      "Demo credit comes from the gateway's mock payment provider. It is not an on-chain " +
      "deposit; verified on-chain funding is shown on the Pay on NEAR page."));
    if (body.funded !== true) {
      out.appendChild(el("p", { class: "notice bad" },
        "The payment provider did not confirm this intent. A NEAR-backed gateway needs a real " +
        "transfer; switch the data source to mock data to explore the flow."));
    }
    var pairs = [["Intent", body.intent_id || "-", "mono"], ["Funded", String(body.funded)]];
    if (body.deposit) {
      pairs.push(["Credited", body.deposit.credited.decimal + " USDC"]);
      pairs.push(["Available", body.deposit.balances.available.decimal + " USDC"]);
    }
    out.appendChild(A.kv(pairs));
  }

  async function scan() {
    var out = $("verdict-out");
    var button = $("btn-scan");
    var account = $("account").value.trim();
    var raw = $("envelope").value;
    var envelope;
    try {
      envelope = JSON.parse(raw);
    } catch (err) {
      fail(out, "The finding is not valid JSON: " + err.message);
      return;
    }
    busy(button, true);
    try {
      if (A.mode() === "mock") {
        await mockScan(envelope);
        return;
      }
      var reservation = await A.api("POST", "/reservations", {
        account_id: account,
        sku: "scan.v1",
        amount_atomic: "1000000",
        idempotency_key: A.randomKey("ui-")
      });
      if (!reservation.ok) throw new Error(A.apiError(reservation));
      var jobId = reservation.json.job_id;
      var started = await A.api("POST", "/jobs/" + encodeURIComponent(jobId) + "/start");
      if (!started.ok) throw new Error(A.apiError(started));
      var result = await A.api("POST", "/jobs/" + encodeURIComponent(jobId) + "/scan", raw);
      if (!result.ok) {
        var refund = await A.api("POST", "/jobs/" + encodeURIComponent(jobId) + "/refunds",
          { reason: "ui: scan did not complete" });
        throw new Error(A.apiError(result) + (refund.ok
          ? " The reservation was refunded; nothing was charged."
          : " Refund the job " + jobId + " to release the reservation."));
      }
      await renderVerdict(result.json.report, result.json.settlement, envelope, "live");
    } catch (err) {
      fail(out, "Scan failed: " + err.message);
    } finally {
      busy(button, false);
    }
  }

  async function mockScan(envelope) {
    var id = $("sample").value;
    var sample = S.envelopes[id].envelope;
    if (JSON.stringify(envelope) !== JSON.stringify(sample)) {
      var out = A.clear($("verdict-out"));
      out.appendChild(el("h3", null, "Edited finding ", A.sourceBadge("mock")));
      out.appendChild(el("p", { class: "notice mock" },
        "Mock mode replays recorded scanner output for the unedited samples only. The gates " +
        "run in the scanner, so an edited finding needs the live API."));
      try {
        var hash = await V.contentHash(envelope);
        out.appendChild(A.kv([["Content hash (computed in this browser)", hash, "mono hash"]]));
      } catch (err) {
        out.appendChild(el("p", { class: "notice bad" }, "Cannot hash: " + err.message));
      }
      return;
    }
    var report = S.recordedReports[id];
    await renderVerdict(report, S.mockSettlement(report.content_hash), envelope, "mock");
  }

  function taxonomyBlock(report) {
    var hits = report.taxonomy_hits || report.taxonomy;
    if (Array.isArray(hits) && hits.length) {
      return el("ul", null, hits.map(function (hit) {
        return el("li", null, typeof hit === "string" ? hit
          : (hit.id || "") + (hit.label ? " - " + hit.label : ""));
      }));
    }
    return el("p", { class: "hint" },
      "scan.v1 reports gate results, not taxonomy classes. Taxonomy hits arrive with the " +
      "triage.v2 SKU (planned).");
  }

  async function renderVerdict(report, settlement, envelope, source) {
    var out = A.clear($("verdict-out"));
    out.appendChild(el("h3", null, "Verdict ", A.sourceBadge(source)));
    if (source === "mock") {
      out.appendChild(el("p", { class: "notice mock" },
        "Recorded scanner output for this sample (ruleset " + report.ruleset_version +
        "); no request was sent and nothing was charged."));
    }
    out.appendChild(el("p", null,
      el("span", { class: "pill " + (DECISION_CLASS[report.decision] || "warn") },
        report.decision), " confidence: ", el("strong", null, report.confidence_band)));

    var recomputed = "";
    try { recomputed = await V.contentHash(envelope); } catch (err) { recomputed = ""; }
    var match = recomputed && recomputed === report.content_hash;
    out.appendChild(A.kv([
      ["Content hash", report.content_hash, "mono hash"],
      ["Recomputed here", recomputed ? (match ? "match" : "MISMATCH: " + recomputed)
        : "not computable for this input"],
      ["Ruleset", report.ruleset_version],
      ["Ruleset hash", report.ruleset_hash, "mono hash"],
      ["Embedded instructions", report.contains_embedded_instructions
        ? "flagged (advisory only; never changes the verdict)" : "none found"]
    ]));

    out.appendChild(el("h3", null, "Taxonomy hits"));
    out.appendChild(taxonomyBlock(report));

    out.appendChild(el("h3", null, "Gate results"));
    var rows = (report.gate_results || []).map(function (g) {
      var cls = g.status === "Pass" ? "ok" : (g.status === "Fail" ? "bad" : "warn");
      return el("tr", null, el("td", { class: "mono" }, g.gate),
        el("td", null, el("span", { class: "pill " + cls }, g.status)), el("td", null, g.reason));
    });
    out.appendChild(el("div", { class: "table-wrap" }, el("table", null,
      el("caption", { class: "hint" }, "Each deterministic gate, in evaluation order"),
      el("thead", null, el("tr", null, el("th", { scope: "col" }, "Gate"),
        el("th", { scope: "col" }, "Status"), el("th", { scope: "col" }, "Reason"))),
      el("tbody", null, rows))));

    if (settlement) {
      out.appendChild(el("h3", null, "Settlement receipt"));
      out.appendChild(A.kv([
        ["Job", settlement.job_id, "mono"],
        ["Charged", settlement.charged.decimal + " USDC"],
        ["Report hash", settlement.report_hash_hex, "mono hash"],
        ["Bound to report", settlement.report_hash_hex === report.content_hash ? "yes" : "NO"],
        ["Available after", settlement.balances.available.decimal + " USDC"]
      ]));
    }
    A.stash("aps-last-scan", { report: report, settlement: settlement, envelope: envelope,
      source: source });
    out.appendChild(el("p", null, el("a", { href: "verify.html" },
      "Check this verdict on the Verify page")));
  }

  A.ready.then(function () {
    var select = $("sample");
    Object.keys(S.envelopes).forEach(function (id) {
      select.appendChild(el("option", { value: id }, S.envelopes[id].label));
    });
    select.addEventListener("change", loadSample);
    loadSample();
    $("btn-credit").addEventListener("click", credit);
    $("btn-scan").addEventListener("click", scan);
  });
})();
