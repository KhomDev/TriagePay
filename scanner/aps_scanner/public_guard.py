"""Leakage guard for the public v2 report.

The scanner stays dependency-free: the shared leakage checker is the
knowledge compiler's stdlib-only module, loaded by path from the
repository layout (or from APS_LEAKAGE_MODULE). If it cannot be loaded the
guard is unavailable and the v2 report is refused (fail closed).

Only free-text fields are scanned (gate reasons, the model's public
rationale, model id, ids). Hash fields are never passed in: a 64-hex
digest looks like an implicit account id to the structural patterns.
"""

import importlib.util
import os
import pathlib

LEAKAGE_MODULE_ENV = "APS_LEAKAGE_MODULE"


class PublicGuardError(RuntimeError):
    """The guard is unavailable or a non-withholdable field leaked."""


def _default_module_path() -> pathlib.Path:
    return (
        pathlib.Path(__file__).resolve().parents[2]
        / "knowledge-compiler"
        / "knowledge_compiler"
        / "leakage.py"
    )


_MODULE = None


def _leakage():
    global _MODULE
    if _MODULE is None:
        path = pathlib.Path(os.environ.get(LEAKAGE_MODULE_ENV) or _default_module_path())
        if not path.is_file():
            raise PublicGuardError("the public-report leakage guard is unavailable")
        spec = importlib.util.spec_from_file_location("aps_public_guard_leakage", path)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        _MODULE = module
    return _MODULE


def leakage_classes(text: str, digests=None) -> list:
    """Pattern classes in the text (empty list when clean)."""
    module = _leakage()
    try:
        return module.scan_public_text(text or "", digests)
    except module.DenylistError as err:
        raise PublicGuardError("the private denylist could not be loaded") from err


def is_clean(text: str, digests=None) -> bool:
    return not leakage_classes(text, digests)
