"""Command-line interface.

    python -m knowledge_compiler compile --source DIR --out FILE
        [--ruleset-id ID] [--version SEMVER] [--timestamp N] [--check]
    python -m knowledge_compiler digest < terms.txt
    python -m knowledge_compiler scan-tree --root DIR [--require-private]

Exit codes: 0 success (and, with --check, byte-identical artifact);
2 validation, leakage, or check mismatch failure. scan-tree exits 1 when
a denylisted term is found in a tracked file (it prints path:line only).
"""

import argparse
import pathlib
import sys

from . import leakage
from .compiler import compile_ruleset, read_artifact, write_artifact


def _build_parser():
    parser = argparse.ArgumentParser(prog="knowledge_compiler")
    subparsers = parser.add_subparsers(dest="command", required=True)
    compile_cmd = subparsers.add_parser("compile", help="compile sources to an artifact")
    compile_cmd.add_argument("--source", required=True, help="source directory")
    compile_cmd.add_argument("--out", required=True, help="artifact output path")
    compile_cmd.add_argument("--ruleset-id", default="rules-v0.synthetic.1")
    compile_cmd.add_argument("--version", default="0.1.0")
    compile_cmd.add_argument(
        "--timestamp",
        type=int,
        default=0,
        help="created_unix stamp (0 keeps builds deterministic)",
    )
    compile_cmd.add_argument(
        "--check",
        action="store_true",
        help="verify an existing artifact is byte-identical to a recompile",
    )
    return parser


LEAKAGE_COMMANDS = ("digest", "scan-tree")


def main(argv=None):
    argv = sys.argv[1:] if argv is None else list(argv)
    if argv and argv[0] in LEAKAGE_COMMANDS:
        # Denylist helpers live in the leakage module (see its docstring).
        return leakage.main(argv)
    args = _build_parser().parse_args(argv)
    try:
        artifact = compile_ruleset(
            pathlib.Path(args.source), args.ruleset_id, args.version, args.timestamp
        )
    except ValueError as err:  # SchemaError and friends
        print(f"compile failed: {err}", file=sys.stderr)
        return 2

    out_path = pathlib.Path(args.out)
    if args.check:
        if not out_path.exists():
            print(f"check failed: {out_path} does not exist", file=sys.stderr)
            return 2
        try:
            read_artifact(out_path)
        except ValueError as err:
            print(f"check failed: existing artifact is invalid: {err}", file=sys.stderr)
            return 2
        fresh = write_artifact(artifact, out_path.with_suffix(".fresh"))
        if fresh.read_bytes() != out_path.read_bytes():
            fresh.unlink()
            print(
                "check failed: the compiled artifact differs from the committed one",
                file=sys.stderr,
            )
            return 2
        fresh.unlink()
        print(f"check ok: {out_path}")
        return 0

    write_artifact(artifact, out_path)
    print(f"compiled {out_path} (sha256 {artifact['payload_sha256']})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
