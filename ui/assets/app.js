/*
 * Shared helpers for the AgentPay Sentinel demo UI.
 *
 * - Settings (API base URL, data source, network) persist per browser in
 *   localStorage when available; every access is guarded.
 * - Data source "auto" uses the live API when GET /health answers and
 *   mock data otherwise. Every rendered result carries a LIVE or MOCK
 *   badge so mock data is never mistaken for real data.
 * - All DOM output goes through el()/text nodes: report text is hostile
 *   input and is never parsed as markup.
 */
(function () {
  "use strict";

  var STORE_KEY = "aps-ui-settings-v1";
  var DEFAULTS = { apiBase: "", mode: "auto", network: "testnet" };

  /* Hosts the UI may call at runtime (also pinned by the page CSP).
   * rpc.{testnet,mainnet}.near.org answered every call with HTTP 429 and a
   * deprecation notice on 2026-11-06 and send no CORS headers; FASTNEAR's
   * free endpoints answer and allow browser origins. */
  var RPC = {
    testnet: "https://test.rpc.fastnear.com",
    mainnet: "https://free.rpc.fastnear.com"
  };
  /* Regular nodes drop old transactions after a few epochs; archival
   * nodes keep them. Used only as a fallback for transaction lookups. */
  var ARCHIVAL_RPC = {
    testnet: "https://archival-rpc.testnet.fastnear.com",
    mainnet: "https://archival-rpc.mainnet.fastnear.com"
  };
  var EXPLORER = {
    testnet: "https://testnet.nearblocks.io",
    mainnet: "https://nearblocks.io"
  };

  function loadSettings() {
    try {
      var raw = window.localStorage.getItem(STORE_KEY);
      var parsed = raw ? JSON.parse(raw) : {};
      return Object.assign({}, DEFAULTS, parsed);
    } catch (err) {
      return Object.assign({}, DEFAULTS);
    }
  }

  function saveSettings(settings) {
    try {
      window.localStorage.setItem(STORE_KEY, JSON.stringify(settings));
    } catch (err) {
      /* storage unavailable: settings last for this page view only */
    }
  }

  var settings = loadSettings();
  var apiUp = false;

  function apiBase() {
    return (settings.apiBase || "").replace(/\/+$/, "");
  }

  function mode() {
    if (settings.mode === "live" || settings.mode === "mock") return settings.mode;
    return apiUp ? "live" : "mock";
  }

  /* Build an element. attrs: plain attributes; children: nodes or text. */
  function el(tag, attrs) {
    var node = document.createElement(tag);
    var key;
    attrs = attrs || {};
    for (key in attrs) {
      if (!Object.prototype.hasOwnProperty.call(attrs, key)) continue;
      if (attrs[key] === null || attrs[key] === undefined || attrs[key] === false) continue;
      if (key === "class") node.className = attrs[key];
      else node.setAttribute(key, attrs[key] === true ? "" : String(attrs[key]));
    }
    for (var i = 2; i < arguments.length; i++) append(node, arguments[i]);
    return node;
  }

  function append(node, child) {
    if (child === null || child === undefined || child === false) return;
    if (Array.isArray(child)) {
      child.forEach(function (c) { append(node, c); });
    } else if (child instanceof Node) {
      node.appendChild(child);
    } else {
      node.appendChild(document.createTextNode(String(child)));
    }
  }

  function clear(node) {
    while (node.firstChild) node.removeChild(node.firstChild);
    return node;
  }

  function sourceBadge(source) {
    return source === "live"
      ? el("span", { class: "pill live" }, "LIVE")
      : el("span", { class: "pill mock" }, "MOCK DATA");
  }

  function kv(pairs) {
    var dl = el("dl", { class: "kv" });
    pairs.forEach(function (pair) {
      dl.appendChild(el("dt", null, pair[0]));
      dl.appendChild(el("dd", pair[2] ? { class: pair[2] } : null, pair[1]));
    });
    return dl;
  }

  /* Render u128 atomic units as a decimal string, display only. */
  function formatAtomic(atomic, decimals) {
    try {
      var value = BigInt(String(atomic));
      var scale = BigInt(10) ** BigInt(decimals);
      var whole = value / scale;
      var frac = (value % scale).toString().padStart(decimals, "0");
      return decimals > 0 ? whole.toString() + "." + frac : whole.toString();
    } catch (err) {
      return String(atomic);
    }
  }

  async function api(method, path, body) {
    var init = { method: method, headers: {} };
    if (body !== undefined) {
      init.headers["content-type"] = "application/json";
      init.body = typeof body === "string" ? body : JSON.stringify(body);
    }
    var res = await fetch(apiBase() + path, init);
    var text = await res.text();
    var json = null;
    try { json = JSON.parse(text); } catch (err) { json = null; }
    return { status: res.status, ok: res.ok, json: json, text: text };
  }

  function apiError(result) {
    if (result.json && result.json.error) {
      return result.json.error.code + ": " + result.json.error.message;
    }
    return "HTTP " + result.status;
  }

  async function probe() {
    var controller = new AbortController();
    var timer = setTimeout(function () { controller.abort(); }, 2500);
    try {
      var res = await fetch(apiBase() + "/health", { signal: controller.signal });
      apiUp = res.ok;
    } catch (err) {
      apiUp = false;
    } finally {
      clearTimeout(timer);
    }
    return apiUp;
  }

  /* One read-only NEAR JSON-RPC call, with a 15 s timeout. The thrown
   * error carries the RPC cause name (for example UNKNOWN_TRANSACTION). */
  async function rpc(network, method, params, archival) {
    var url = (archival ? ARCHIVAL_RPC : RPC)[network];
    if (!url) throw new Error("unknown network " + network);
    var controller = new AbortController();
    var timer = setTimeout(function () { controller.abort(); }, 15000);
    var json;
    try {
      var res = await fetch(url, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ jsonrpc: "2.0", id: "aps-ui", method: method, params: params }),
        signal: controller.signal
      });
      json = await res.json();
    } catch (err) {
      throw new Error(err.name === "AbortError" ? "RPC timed out" : "RPC unreachable");
    } finally {
      clearTimeout(timer);
    }
    if (json.error) {
      var cause = json.error.cause && json.error.cause.name;
      var error = new Error((cause || json.error.name || "RPC error") + ": " +
        (json.error.data || json.error.message || ""));
      error.cause_name = cause || "";
      throw error;
    }
    return json.result;
  }

  /* call_function view at final finality; returns the decoded JSON. */
  async function viewCall(network, contractId, methodName, args) {
    var argsJson = JSON.stringify(args || {});
    var bytes = new TextEncoder().encode(argsJson);
    var binary = "";
    bytes.forEach(function (b) { binary += String.fromCharCode(b); });
    var result = await rpc(network, "query", {
      request_type: "call_function",
      finality: "final",
      account_id: contractId,
      method_name: methodName,
      args_base64: btoa(binary)
    });
    /* A failed view (for example MethodNotFound) arrives as result.error. */
    if (result.error) throw new Error(methodName + " failed: " + result.error);
    if (!Array.isArray(result.result)) throw new Error(methodName + " returned no value");
    var text = new TextDecoder().decode(new Uint8Array(result.result));
    try {
      return JSON.parse(text);
    } catch (err) {
      throw new Error(methodName + " returned a value that is not JSON");
    }
  }

  function explorerTx(network, hash) {
    return EXPLORER[network] + "/txns/" + encodeURIComponent(hash);
  }

  function explorerAccount(network, account) {
    return EXPLORER[network] + "/address/" + encodeURIComponent(account);
  }

  function renderStatus() {
    var status = document.getElementById("api-status");
    if (!status) return;
    clear(status);
    status.appendChild(el("span", { class: apiUp ? "pill ok" : "pill warn" },
      apiUp ? "API reachable" : "API not reachable"));
    status.appendChild(document.createTextNode(" "));
    status.appendChild(sourceBadge(mode()));
  }

  function wireSettings() {
    var base = document.getElementById("api-base");
    var modeSel = document.getElementById("data-mode");
    if (base) {
      base.value = settings.apiBase;
      base.addEventListener("change", async function () {
        settings.apiBase = base.value.trim();
        saveSettings(settings);
        await probe();
        renderStatus();
        document.dispatchEvent(new CustomEvent("aps:settings"));
      });
    }
    if (modeSel) {
      modeSel.value = settings.mode;
      modeSel.addEventListener("change", function () {
        settings.mode = modeSel.value;
        saveSettings(settings);
        renderStatus();
        document.dispatchEvent(new CustomEvent("aps:settings"));
      });
    }
  }

  function stash(key, value) {
    try { window.sessionStorage.setItem(key, JSON.stringify(value)); return true; }
    catch (err) { return false; }
  }

  function unstash(key) {
    try {
      var raw = window.sessionStorage.getItem(key);
      return raw ? JSON.parse(raw) : null;
    } catch (err) {
      return null;
    }
  }

  function randomKey(prefix) {
    var bytes = new Uint8Array(8);
    crypto.getRandomValues(bytes);
    return prefix + Array.from(bytes, function (b) {
      return b.toString(16).padStart(2, "0");
    }).join("");
  }

  var ready = (async function () {
    await new Promise(function (resolve) {
      if (document.readyState === "loading") {
        document.addEventListener("DOMContentLoaded", resolve, { once: true });
      } else {
        resolve();
      }
    });
    wireSettings();
    await probe();
    renderStatus();
  })();

  window.APS = {
    ready: ready,
    settings: function () { return settings; },
    mode: mode,
    api: api,
    apiError: apiError,
    rpc: rpc,
    viewCall: viewCall,
    explorerTx: explorerTx,
    explorerAccount: explorerAccount,
    el: el,
    clear: clear,
    kv: kv,
    sourceBadge: sourceBadge,
    formatAtomic: formatAtomic,
    stash: stash,
    unstash: unstash,
    randomKey: randomKey,
    RPC: RPC
  };
})();
