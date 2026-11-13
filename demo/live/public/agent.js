/* Agent passport. Text nodes only. Limits are the wallet view. Rows are the indexer. */
(function () {
  "use strict";
  var root = document.getElementById("main");

  function el(tag, attrs) {
    var node = document.createElement(tag);
    var kids = Array.prototype.slice.call(arguments, 2);
    if (attrs) Object.keys(attrs).forEach(function (key) {
      if (key === "class") node.className = String(attrs[key]);
      else if (attrs[key] != null) node.setAttribute(key, String(attrs[key]));
    });
    kids.forEach(function (child) {
      if (child == null || child === false) return;
      node.appendChild(typeof child === "string" ? document.createTextNode(child) : child);
    });
    return node;
  }

  function accountFromPath() {
    var parts = location.pathname.split("/").filter(Boolean);
    if (parts[0] === "agent" && parts[1]) return parts[1];
    return new URLSearchParams(location.search).get("account") || "";
  }

  function paint(data) {
    var card = el("article", { class: "panel passport" });
    card.appendChild(el("p", { class: "eyebrow" }, "Agent passport"));
    card.appendChild(el("h1", null, data.accountId || "Agent"));
    if (!data.found) card.appendChild(el("p", { class: "notice" }, "This wallet has no agent with that id."));
    card.appendChild(el("p", null, data.active ? "Active" : "Not active"));
    var list = el("dl", { class: "passport-dl" });
    [["Per call", data.perCall], ["Daily", data.daily], ["Spent in window", data.spent], ["Approval threshold", data.approvalThreshold]].forEach(function (pair) {
      if (!pair[1]) return;
      list.appendChild(el("dt", null, pair[0]));
      list.appendChild(el("dd", null, pair[1]));
    });
    card.appendChild(list);
    card.appendChild(el("p", null, "Allowlist: " + ((data.allowlist || []).join(", ") || "none")));
    var counts = data.counts || {};
    card.appendChild(el("p", null, "In the indexed transactions: " + (counts.refusals || 0) + " refusals, " + (counts.paid || 0) + " payments, " + (counts.approved || 0) + " approvals, " + (counts.revokes || 0) + " revokes."));
    var source = data.source || {};
    card.appendChild(el("p", { class: "hint" }, "Limits: " + (source.limits || "") + ". Transactions: " + (source.transactions || "") + ". Error codes: " + (source.errorCodes || "") + "."));
    if (data.explorerUrl) card.appendChild(el("p", null, el("a", { href: data.explorerUrl, rel: "noopener", target: "_blank" }, "Account on the explorer")));
    var feed = el("ol", { class: "feed" });
    (data.events || []).forEach(function (row) {
      var link = row.explorerUrl ? el("a", { href: row.explorerUrl, rel: "noopener", target: "_blank" }, "transaction") : null;
      feed.appendChild(el("li", null, (row.label || "transaction") + (row.errorCode ? " " + row.errorCode : ""), link ? " " : "", link || ""));
    });
    card.appendChild(feed);
    var jsonUrl = "/api/passport?account=" + encodeURIComponent(data.accountId || "");
    card.appendChild(el("p", null, el("a", { href: jsonUrl }, "JSON for other services")));
    root.appendChild(card);
  }

  var account = accountFromPath();
  var url = "/api/passport" + (account ? "?account=" + encodeURIComponent(account) : "");
  fetch(url).then(function (response) { return response.json(); }).then(function (body) {
    if (body && body.error) {
      root.appendChild(el("p", { class: "notice bad" }, body.error.message || "The passport could not be read."));
      return;
    }
    paint(body);
  }).catch(function () {
    root.appendChild(el("p", { class: "notice bad" }, "The passport could not be read."));
  });
})();
