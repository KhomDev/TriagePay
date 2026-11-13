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
    if (!data || data.error || data.kind === "unknown") {
      root.appendChild(el("p", { class: "notice bad" }, (data && data.error && data.error.message) || "That transaction is not a pay call I can replay."));
      return;
    }
    var stage = el("section", { class: "theatre", id: "theatre" });
    var list = el("ol", { class: "pipeline", id: "rep-steps" });
    var stampWord = data.kind === "held" ? "HELD" : data.kind === "settled" ? "SETTLED" : "REFUSED";
    var stamp = el("p", { class: "stamp", id: "rep-stamp" }, stampWord);
    var steps = (data.checks || []).slice(0, 4);
    if (!steps.length) {
      steps = [
        { name: "Reads the finding", result: data.injection || "The transaction calls pay." },
        { name: "Injection", result: data.payee || "" },
        { name: "Signs pay", result: data.amountDisplay || "" },
        { name: "Contract", result: data.errorCode || stampWord },
      ];
    }
    stage.appendChild(el("p", { class: "eyebrow" }, "Replay"));
    stage.appendChild(el("h1", null, "The agent signs. The contract decides."));
    stage.appendChild(list);
    stage.appendChild(stamp);
    if (data.errorCode) stage.appendChild(el("p", { class: "mono" }, data.errorCode));
    if (data.explorerUrl) stage.appendChild(el("p", null, el("a", { href: data.explorerUrl, rel: "noopener", target: "_blank" }, "Open the transaction")));
    root.appendChild(stage);
    stamp.hidden = true;
    function show(index) {
      var check = steps[index];
      var item = el("li", { class: "step is-on" },
        el("span", { class: "kicker" }, "0" + (index + 1) + " · " + (check.name || "Step")),
        el("p", { class: "trace" }, check.result || "")
      );
      list.appendChild(item);
    }
    var count = Math.min(4, steps.length);
    if (reduce) {
      for (var n = 0; n < count; n++) show(n);
      stamp.hidden = false;
      return;
    }
    for (var i = 0; i < count; i++) {
      show(i);
      await wait(350);
    }
    stamp.hidden = false;
    stamp.classList.add("is-in");
  }

  var tx = txFromPath();
  if (!tx) {
    root.appendChild(el("p", { class: "hint" }, "Add a transaction hash to the URL."));
    return;
  }
  fetch("/api/replay?tx=" + encodeURIComponent(tx))
    .then(function (response) { return response.json().then(function (body) { return { ok: response.ok, body: body }; }); })
    .then(function (result) { return play(result.ok ? result.body : { error: (result.body && result.body.error) || { message: "That transaction is not a pay call I can replay." } }); })
    .catch(function () {
      root.appendChild(el("p", { class: "notice bad" }, "The replay could not be loaded."));
    });
})();
