(function () {
  "use strict";
  var node = document.getElementById("status");
  if (!node) return;
  fetch("/health").then(function (response) {
    return response.json().then(function (body) { return { ok: response.ok && body && body.ok, body: body }; });
  }).then(function (result) {
    node.className = "status " + (result.ok ? "is-up" : "is-down");
    node.textContent = result.ok ? "Live on NEAR mainnet" : "Service degraded";
  }).catch(function () {
    node.className = "status is-down";
    node.textContent = "Status unavailable";
  });
})();
