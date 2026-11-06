"""Make the sibling aps_scanner package importable (stdlib only, no install).

The repository ships its Python packages side by side without an install
step. When ``aps_scanner`` is not already importable, the sibling
``scanner/`` directory of this checkout (or APS_SCANNER_DIR) is appended to
sys.path.
"""

import os
import pathlib
import sys


def ensure_scanner_importable() -> None:
    try:
        import aps_scanner  # noqa: F401

        return
    except ImportError:
        pass
    override = os.environ.get("APS_SCANNER_DIR", "").strip()
    candidate = (
        pathlib.Path(override)
        if override
        else pathlib.Path(__file__).resolve().parents[2] / "scanner"
    )
    if (candidate / "aps_scanner" / "__init__.py").is_file():
        sys.path.append(str(candidate))
    import aps_scanner  # noqa: F401  (raises ImportError when still missing)


ensure_scanner_importable()
