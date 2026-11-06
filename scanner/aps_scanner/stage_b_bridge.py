"""Locate the triage-model package and build the configured Stage B judge.

The scanner never depends on triage-model at import time. Only the
``triage.v2`` SKU reaches this module. The package is found through
APS_TRIAGE_MODEL_DIR or the sibling ``triage-model/`` directory of this
checkout. When it cannot be imported, or no model is configured (no live
switch and no cassette directory), the judge reports Stage B as
"unavailable", which the combiner treats as needs_evidence (fail closed).
"""

import os
import pathlib
import sys

TRIAGE_MODEL_DIR_ENV = "APS_TRIAGE_MODEL_DIR"


def _unavailable(failure):
    def judge(_request):
        return {"status": "unavailable", "failure": failure}

    return judge


def load_judge(environ=None):
    environ = os.environ if environ is None else environ
    override = environ.get(TRIAGE_MODEL_DIR_ENV, "").strip()
    directory = (
        pathlib.Path(override)
        if override
        else pathlib.Path(__file__).resolve().parents[2] / "triage-model"
    )
    if not (directory / "triage_model" / "__init__.py").is_file():
        return _unavailable("model_package_missing")
    if str(directory) not in sys.path:
        sys.path.append(str(directory))
    try:
        from triage_model.judge import judge_from_env
    except ImportError:
        return _unavailable("model_package_missing")
    judge = judge_from_env(environ)
    return judge if judge is not None else _unavailable("model_not_configured")
