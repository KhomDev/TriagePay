"""Container healthcheck: GET /health on the local API (stdlib only).

Exit 0 when the API answers {"status": "ok"}, 1 otherwise. Logs nothing on
success and only the failure class on error, never a body.
"""

import json
import os
import sys
import urllib.request


def main() -> int:
    port = os.environ.get("APS_PORT", "8080")
    try:
        with urllib.request.urlopen(f"http://127.0.0.1:{port}/health", timeout=4) as res:
            ok = res.status == 200 and json.load(res).get("status") == "ok"
    except Exception as err:  # any failure means unhealthy
        print(f"healthcheck: {type(err).__name__}", file=sys.stderr)
        return 1
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
