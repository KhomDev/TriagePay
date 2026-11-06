"""Command-line interface.

    python -m knowledge_compiler compile --source DIR --out FILE
        [--ruleset-id ID] [--version SEMVER] [--timestamp N] [--check]
    python -m knowledge_compiler compile --check     (every committed build)
    python -m knowledge_compiler compile --all       (rewrite every committed build)
    python -m knowledge_compiler digest < terms.txt
    python -m knowledge_compiler scan-tree --root DIR [--require-private]
    python -m knowledge_compiler compile-private [--out FILE]

compile-private reads the operator's sources from APS_PRIVATE_SOURCES and
writes rules-v1.private to --out or APS_PRIVATE_RULESET_OUT; both must lie
outside the repository, and only the manifest hash is printed.

Exit codes: 0 success (and, with --check, byte-identical artifact);
2 validation, leakage, or check mismatch failure. scan-tree exits 1 when
a denylisted term is found in a tracked file (it prints path:line only).
"""

import argparse
import pathlib
import sys

from . import leakage, private
from .compiler import compile_ruleset, read_artifact, write_artifact

PACKAGE_ROOT = pathlib.Path(__file__).resolve().parent.parent

# The committed artifacts: (source dir, artifact path, ruleset id, version),
# paths relative to the knowledge-compiler directory.
COMMITTED_BUILDS = (
    (
        "samples/synthetic",
        "../scanner/rulesets/rules-v0.synthetic.1.json",
        "rules-v0.synthetic.1",
        "0.1.0",
    ),
    (
        "samples/public-v1",
        "../scanner/rulesets/rules-v1.public.json",
        "rules-v1.public",
        "1.0.0",
    ),
)


def _build_parser():
    parser = argparse.ArgumentParser(prog="knowledge_compiler")
    subparsers = parser.add_subparsers(dest="command", required=True)
    compile_cmd = subparsers.add_parser("compile", help="compile sources to an artifact")
    compile_cmd.add_argument("--source", help="source directory")
    compile_cmd.add_argument("--out", help="artifact output path")
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
        help="verify an existing artifact is byte-identical to a recompile "
        "(without --source/--out: every committed build)",
    )
    compile_cmd.add_argument(
        "--all", action="store_true", help="rewrite every committed build in place"
    )
    private_cmd = subparsers.add_parser(
        "compile-private", help="compile rules-v1.private outside the repository"
    )
    private_cmd.add_argument("--out", help=f"output file (default: ${private.OUT_ENV})")
    return parser


LEAKAGE_COMMANDS = ("digest", "scan-tree")


def _check_one(artifact, out_path):
    if not out_path.exists():
        print(f"check failed: {out_path.name} does not exist", file=sys.stderr)
        return 2
    try:
        read_artifact(out_path)
    except ValueError as err:
        print(f"check failed: existing artifact is invalid: {err}", file=sys.stderr)
        return 2
    fresh = write_artifact(artifact, out_path.with_suffix(".fresh"))
    same = fresh.read_bytes() == out_path.read_bytes()
    fresh.unlink()
    if not same:
        print(
            f"check failed: the compiled artifact differs from the committed {out_path.name}",
            file=sys.stderr,
        )
        return 2
    print(f"check ok: {out_path.name}")
    return 0


def _committed(check):
    status = 0
    for source, out, ruleset_id, version in COMMITTED_BUILDS:
        try:
            artifact = compile_ruleset(PACKAGE_ROOT / source, ruleset_id, version)
        except ValueError as err:
            print(f"compile failed ({ruleset_id}): {err}", file=sys.stderr)
            return 2
        out_path = (PACKAGE_ROOT / out).resolve()
        if check:
            status = max(status, _check_one(artifact, out_path))
        else:
            write_artifact(artifact, out_path)
            print(f"compiled {out_path.name} (sha256 {artifact['payload_sha256']})")
    return status


def main(argv=None):
    argv = sys.argv[1:] if argv is None else list(argv)
    if argv and argv[0] in LEAKAGE_COMMANDS:
        # Denylist helpers live in the leakage module (see its docstring).
        return leakage.main(argv)
    args = _build_parser().parse_args(argv)
    if args.command == "compile-private":
        return private.main(args.out)
    if args.source is None and args.out is None:
        if args.check or args.all:
            return _committed(check=args.check)
        print("compile needs --source and --out (or --check / --all)", file=sys.stderr)
        return 2
    if args.source is None or args.out is None:
        print("compile needs both --source and --out", file=sys.stderr)
        return 2
    try:
        artifact = compile_ruleset(
            pathlib.Path(args.source), args.ruleset_id, args.version, args.timestamp
        )
    except ValueError as err:  # SchemaError and friends
        print(f"compile failed: {err}", file=sys.stderr)
        return 2

    out_path = pathlib.Path(args.out)
    if args.check:
        return _check_one(artifact, out_path)

    write_artifact(artifact, out_path)
    print(f"compiled {out_path} (sha256 {artifact['payload_sha256']})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
