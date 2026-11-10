/*
 * Client-side verdict checks for scan.v1 reports.
 *
 * content_hash is SHA-256 of the canonical JSON of the NORMALIZED envelope
 * (scanner/aps_scanner/report.py: json.dumps(sort_keys=True,
 * separators=(",", ":"), ensure_ascii=True)). This file re-implements the
 * scanner's normalization (normalize.py) so the hash can be recomputed in
 * the browser from the raw envelope:
 *   NFC fold -> controls (Cc) to U+FFFD -> drop zero-width chars ->
 *   collapse Unicode whitespace runs to one space (text fields only).
 * The embedded-instruction flag is part of the hashed payload, so its
 * patterns are mirrored too. Parity with the Python scanner is checked by
 * ui/tests/test_verdict_parity.py (runs when Node is installed).
 *
 * The source is ASCII only: character classes are built from code points.
 * Runs in browsers and in Node (globalThis.crypto.subtle).
 */
(function (root) {
  "use strict";

  var TEXT_FIELDS = [
    "title", "claimed_impact", "mechanism", "attacker_capability", "preconditions",
    "component_class", "scope_statement", "deployed_state_evidence",
    "public_rules_snapshot", "reproduction_summary"
  ];
  var OPTIONAL_FIELDS = ["known_issue_declaration", "code_excerpt"];

  function cp(code) { return String.fromCharCode(code); }

  /* A regex character class from code points and [low, high] ranges. */
  function charClass(parts, suffix, flags) {
    var body = parts.map(function (p) {
      return Array.isArray(p) ? cp(p[0]) + "-" + cp(p[1]) : cp(p);
    }).join("");
    return new RegExp("[" + body + "]" + suffix, flags);
  }

  var REPLACEMENT = cp(0xfffd);
  /* Unicode category Cc: the C0 and C1 controls. */
  var CONTROLS = charClass([[0x00, 0x1f], [0x7f, 0x9f]], "", "g");
  var ZERO_WIDTH = charClass([0x200b, 0x200c, 0x200d, 0xfeff], "", "g");
  /* Python str \s once controls are gone: the str.isspace() code points. */
  var WHITESPACE_RUN = charClass([0x20, 0xa0, 0x1680, [0x2000, 0x200a], 0x2028, 0x2029,
    0x202f, 0x205f, 0x3000], "+", "g");
  /* Python ensure_ascii escapes every UTF-16 unit outside space..tilde. */
  var NON_ASCII_UNIT = charClass([[0x7f, 0xffff]], "", "g");

  var INSTRUCTION_PATTERNS = [
    /ignore (all )?(previous|prior) instructions/iu,
    /disregard (all )?(previous|prior)/iu,
    /system prompt/iu,
    /you are now/iu,
    /act as (a |an )?(different|new)/iu,
    /<\|im_start\|>/iu,
    /todo:? for the (scanner|model|agent)/iu
  ];
  var FENCED_SYSTEM = /```[^`\n]*system/iu;

  function scrubText(text) {
    return text.normalize("NFC").replace(CONTROLS, REPLACEMENT).replace(ZERO_WIDTH, "")
      .replace(WHITESPACE_RUN, " ");
  }

  function scrubCode(text) {
    return text.normalize("NFC").replace(CONTROLS, REPLACEMENT).replace(ZERO_WIDTH, "");
  }

  function hasEmbeddedInstructions(text) {
    if (!text) return false;
    for (var i = 0; i < INSTRUCTION_PATTERNS.length; i++) {
      if (INSTRUCTION_PATTERNS[i].test(text)) return true;
    }
    return FENCED_SYSTEM.test(text);
  }

  /* Normalize a raw envelope object. Throws on a shape the scanner's
   * loader or schema would reject outright (mistyped fields). */
  function normalizeEnvelope(env) {
    if (!env || typeof env !== "object" || Array.isArray(env)) {
      throw new Error("the envelope must be a JSON object");
    }
    var out = {};
    TEXT_FIELDS.forEach(function (name) {
      /* The scanner's loader defaults an absent field to "". */
      var value = env[name] === undefined ? "" : env[name];
      if (typeof value !== "string") throw new Error("field " + name + " must be a string");
      out[name] = scrubText(value);
    });
    OPTIONAL_FIELDS.forEach(function (name) {
      if (env[name] !== null && env[name] !== undefined && typeof env[name] !== "string") {
        throw new Error("field " + name + " must be a string or null");
      }
    });
    out.known_issue_declaration =
      typeof env.known_issue_declaration === "string" ? scrubText(env.known_issue_declaration) : null;
    var refs = env.evidence_refs === undefined ? [] : env.evidence_refs;
    if (!Array.isArray(refs)) throw new Error("evidence_refs must be a list of strings");
    out.evidence_refs = refs.map(function (ref) {
      if (typeof ref !== "string") throw new Error("evidence_refs must hold strings");
      return scrubText(ref);
    });
    out.code_excerpt = typeof env.code_excerpt === "string" ? scrubCode(env.code_excerpt) : null;

    var texts = TEXT_FIELDS.map(function (name) { return out[name]; });
    if (out.known_issue_declaration !== null) texts.push(out.known_issue_declaration);
    if (out.code_excerpt !== null) texts.push(out.code_excerpt);
    texts = texts.concat(out.evidence_refs);
    out.contains_embedded_instructions = texts.some(hasEmbeddedInstructions);
    return out;
  }

  /* json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True) */
  function pythonJson(value) {
    if (value === null) return "null";
    if (typeof value === "boolean") return value ? "true" : "false";
    if (typeof value === "number") {
      if (!Number.isInteger(value)) throw new Error("only integers are supported");
      return String(value);
    }
    if (typeof value === "string") {
      return JSON.stringify(value).replace(NON_ASCII_UNIT, function (ch) {
        return "\\" + "u" + ch.charCodeAt(0).toString(16).padStart(4, "0");
      });
    }
    if (Array.isArray(value)) return "[" + value.map(pythonJson).join(",") + "]";
    var keys = Object.keys(value).sort();
    return "{" + keys.map(function (k) { return pythonJson(k) + ":" + pythonJson(value[k]); })
      .join(",") + "}";
  }

  async function sha256Hex(text) {
    var digest = await root.crypto.subtle.digest("SHA-256", new TextEncoder().encode(text));
    return Array.from(new Uint8Array(digest), function (b) {
      return b.toString(16).padStart(2, "0");
    }).join("");
  }

  async function contentHash(envelope) {
    return sha256Hex(pythonJson(normalizeEnvelope(envelope)));
  }

  var REPORT_FIELDS = [
    "confidence_band", "contains_embedded_instructions", "content_hash", "decision",
    "failed_gates", "gate_results", "missing_evidence", "ruleset_hash", "ruleset_version"
  ];
  var DECISIONS = ["supported", "needs_evidence", "held", "rejected"];
  var BANDS = ["high", "medium", "low"];
  var STATUSES = ["Pass", "Fail", "NeedsEvidence"];
  var HEX64 = /^[0-9a-f]{64}$/;

  function sameList(a, b) {
    return Array.isArray(a) && a.length === b.length &&
      a.every(function (v, i) { return v === b[i]; });
  }

  /* Structural and consistency checks for a report. Each check is
   * {name, status: "pass" | "fail" | "skip", detail}. */
  function checkReport(report, settlement) {
    var checks = [];
    function add(name, ok, detail) {
      checks.push({ name: name, status: ok ? "pass" : "fail", detail: detail || "" });
    }
    if (!report || typeof report !== "object" || Array.isArray(report)) {
      add("report is a JSON object", false, "no report object found");
      return checks;
    }
    var keys = Object.keys(report).sort();
    add("exactly the nine public fields", sameList(keys, REPORT_FIELDS),
      "fields: " + keys.join(", "));
    add("decision is a known value", DECISIONS.indexOf(report.decision) >= 0,
      String(report.decision));
    add("confidence band is a known value", BANDS.indexOf(report.confidence_band) >= 0,
      String(report.confidence_band));
    add("content_hash is 64 lowercase hex", HEX64.test(String(report.content_hash)));
    add("ruleset_hash is 64 lowercase hex", HEX64.test(String(report.ruleset_hash)));

    var gates = Array.isArray(report.gate_results) ? report.gate_results : null;
    var gatesOk = !!gates && gates.every(function (g) {
      return g && typeof g.gate === "string" && typeof g.reason === "string" &&
        STATUSES.indexOf(g.status) >= 0;
    });
    add("gate results are well formed", gatesOk);
    if (gatesOk) {
      var failed = gates.filter(function (g) { return g.status === "Fail"; })
        .map(function (g) { return g.gate; });
      var missing = gates.filter(function (g) { return g.status === "NeedsEvidence"; })
        .map(function (g) { return g.gate; });
      add("failed_gates matches the Fail results", sameList(report.failed_gates, failed));
      add("missing_evidence matches the NeedsEvidence results",
        sameList(report.missing_evidence, missing));
      var expected = failed.length ? ["rejected"]
        : (missing.length ? ["needs_evidence", "held"] : ["supported", "held"]);
      add("decision follows the gate results", expected.indexOf(report.decision) >= 0,
        "expected " + expected.join(" or ") + ", got " + report.decision);
    }
    if (settlement && typeof settlement === "object") {
      add("settlement is bound to this report",
        settlement.report_hash_hex === report.content_hash,
        "settlement report_hash_hex " + settlement.report_hash_hex);
    } else {
      checks.push({ name: "settlement is bound to this report", status: "skip",
        detail: "no settlement in the pasted JSON" });
    }
    return checks;
  }

  var api = {
    normalizeEnvelope: normalizeEnvelope,
    pythonJson: pythonJson,
    sha256Hex: sha256Hex,
    contentHash: contentHash,
    checkReport: checkReport,
    hasEmbeddedInstructions: hasEmbeddedInstructions
  };
  root.APSVerdict = api;
  if (typeof module !== "undefined" && module.exports) module.exports = api;
})(typeof window !== "undefined" ? window : globalThis);
