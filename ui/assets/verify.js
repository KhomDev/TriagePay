/* Verify a verdict: structural checks and a client-side content hash. */
(function () {
  "use strict";
  var A = window.APS;
  var V = window.APSVerdict;
  var el = A.el;

  function $(id) { return document.getElementById(id); }

  function pill(status) {
    var cls = status === "pass" ? "ok" : (status === "fail" ? "bad" : "warn");
    return el("span", { class: "pill " + cls }, status);
  }

  async function verify(event) {
    event.preventDefault();
    var out = A.clear($("verify-out"));
    var parsed;
    try {
      parsed = JSON.parse($("v-report").value);
    } catch (err) {
      out.appendChild(el("p", { class: "notice bad", role: "alert" },
        "The report is not valid JSON: " + err.message));
      return;
    }
    var report = parsed && parsed.report ? parsed.report : parsed;
    var settlement = parsed && parsed.settlement ? parsed.settlement : null;
    var checks = V.checkReport(report, settlement);

    var envelopeText = $("v-envelope").value.trim();
    if (envelopeText) {
      try {
        var hash = await V.contentHash(JSON.parse(envelopeText));
        checks.push({ name: "content_hash recomputed from the envelope",
          status: hash === report.content_hash ? "pass" : "fail", detail: "computed " + hash });
      } catch (err) {
        checks.push({ name: "content_hash recomputed from the envelope", status: "fail",
          detail: err.message });
      }
    } else {
      checks.push({ name: "content_hash recomputed from the envelope", status: "skip",
        detail: "paste the envelope to recompute" });
    }

    var failed = checks.filter(function (c) { return c.status === "fail"; }).length;
    out.appendChild(el("p", null, "Result: ", el("span", { class: failed ? "pill bad" : "pill ok" },
      failed ? failed + " check(s) failed" : "all checks passed")));
    out.appendChild(el("div", { class: "table-wrap" }, el("table", null,
      el("caption", { class: "hint" }, "Checks run in this browser; nothing is sent anywhere"),
      el("thead", null, el("tr", null, el("th", { scope: "col" }, "Check"),
        el("th", { scope: "col" }, "Result"), el("th", { scope: "col" }, "Detail"))),
      el("tbody", null, checks.map(function (c) {
        return el("tr", null, el("td", null, c.name), el("td", null, pill(c.status)),
          el("td", { class: "mono hash" }, c.detail));
      })))));
    out.appendChild(el("p", { class: "hint" },
      "What this proves: the report is well formed, internally consistent, bound to its " +
      "settlement, and (with the envelope) hashes the exact input that was scanned. It does " +
      "not re-run the gates; run the scanner with the same ruleset hash for that."));
  }

  function loadLast() {
    var last = A.unstash("aps-last-scan");
    var note = $("v-note");
    A.clear(note);
    if (!last) {
      note.appendChild(el("span", { class: "hint" }, "No scan from Try it in this tab yet."));
      return;
    }
    $("v-report").value = JSON.stringify({ settlement: last.settlement, report: last.report },
      null, 2);
    $("v-envelope").value = JSON.stringify(last.envelope, null, 2);
    note.appendChild(el("span", null, "Loaded the last scan ", A.sourceBadge(last.source)));
  }

  A.ready.then(function () {
    $("verify-form").addEventListener("submit", verify);
    $("btn-last").addEventListener("click", loadLast);
  });
})();
