"""aps-knowledge-compiler: private knowledge to opaque rulesets.

Compiles offline knowledge sources into a versioned, hash-addressed, and
deliberately opaque ruleset artifact that aps-scanner executes
data-driven. The public repository builds and tests with a committed
sample ruleset; real private sources never enter the repository, and the
compiler refuses (fails closed) to emit any artifact whose serialized
form carries a private marker: protocol names, private-workspace paths,
URLs outside an allowlist, NEAR-style account ids, or secret-like strings.
"""

from .compat import check
from .compiler import compile_ruleset, write_artifact
from .leakage import scan_for_leakage
from .schema import (
    BRANCH_STATUSES,
    EVALUATORS,
    MIGRATION_FAMILIES,
    SchemaError,
    validate_artifact,
    validate_source,
)

__version__ = "0.1.0"

__all__ = [
    "check",
    "compile_ruleset",
    "write_artifact",
    "scan_for_leakage",
    "SchemaError",
    "validate_source",
    "validate_artifact",
    "EVALUATORS",
    "BRANCH_STATUSES",
    "MIGRATION_FAMILIES",
]
