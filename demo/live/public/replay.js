/* Replay theatre. Text nodes only. About ten seconds, or instant when motion is reduced. */
(function () {
  "use strict";
  var reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
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

  function txFromPath() {
    var parts = location.pathname.split("/").filter(Boolean);
    if (parts[0] === "replay" && parts[1]) return parts[1];
    return new URLSearchParams(location.search).get("tx") || "";
  }

  function wait(ms) {
    if (reduce || ms <= 0) return Promise.resolve();
    return new Promise(function (resolve) { setTimeout(resolve, ms); });
  }

  async function play(data) {
    var stage = el("section", { class: "theatre", id: "theatre" });
    var kicker = el("p", { class: "eyebrow" }, "Replay");
    var injection = el("p", { class: "theatre-line", id: "rep-injection" });
    var reasoning = el("p", { class: "theatre-line", id: "rep-reason" });
    var checks = el("ol", { class: "theatre-checks", id: "rep-checks" });
    var stamp = el("p", { class: "stamp", id: "rep-stamp" }, data.kind === "held" ? "HELD" : data.kind === "settled" ? "SETTLED" : data.kind === "unknown" ? "UNRECORDED" : "REFUSED");
    stage.appendChild(kicker);
    stage.appendChild(el("h1", null, data.kind === "unknown" ? "No replay stored" : "Watch the wallet decide"));
    stage.appendChild(injection);
    stage.appendChild(reasoning);
    stage.appendChild(checks);
    stage.appendChild(stamp);
    if (data.explorerUrl) stage.appendChild(el("p", null, el("a", { href: data.explorerUrl, rel: "noopener", target: "_blank" }, "Transaction on the explorer")));
    if (data.note) stage.appendChild(el("p", { class: "hint" }, data.note));
    if (data.source) stage.appendChild(el("p", { class: "hint" }, "Record source: " + data.source + "."));
    stage.appendChild(el("p", { class: "hint" }, "This page is the shareable URL. Record the screen from here."));
    root.appendChild(stage);

    var lines = [
      function () { injection.textContent = data.injection || "No injected text is stored for this transaction."; },
      function () { reasoning.textContent = data.reasoning || (data.agentMode === "scripted" ? "Scripted agent. No model reasoning was stored." : "No reasoning is stored."); },
    ];
    (data.checks || []).forEach(function (check) {
      lines.push(function () {
        checks.appendChild(el("li", null, (check.name || "Check") + ": " + (check.result || "")));
      });
    });
    lines.push(function () { stamp.classList.add("is-in"); });
    if (reduce) {
      lines.forEach(function (step) { step(); });
      return;
    }
    for (var i = 0; i < lines.length; i++) {
      lines[i]();
      await wait(i === lines.length - 1 ? 1800 : 1600);
    }
  }

  var tx = txFromPath();
  if (!tx) {
    root.appendChild(el("p", { class: "hint" }, "Add a transaction hash to the URL."));
    return;
  }
  fetch("/api/replay?tx=" + encodeURIComponent(tx))
    .then(function (response) { return response.json(); })
    .then(play)
    .catch(function () {
      root.appendChild(el("p", { class: "notice bad" }, "The replay could not be loaded."));
    });
})();
