"""Command line interface: ``tablegen build | graph | check``."""

from __future__ import annotations

import argparse
import shutil
import sys
from datetime import datetime
from pathlib import Path

from . import __version__
from .check import CheckInputError, check_directory
from .emit_cpp import DEFAULT_ASSET_BASE, DEFAULT_ASSET_NAME, emit_cpp, generated_member_names
from .emit_json import emit_json
from .errors import ErrorCollector, ValidationErrors
from .excel import load_model
from .graph import emit_graph
from .i18n import SUPPORTED, set_language, tr
from .schema import IDENTIFIER_RE

PLUGIN_ASSET_BASE = "UTableGenAssetBase"
PLUGIN_ASSET_BASE_HEADER = "TableGenAssetBase.h"
PLUGIN_RUNTIME_HEADER = "TableGenRuntime.h"


def create_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="tablegen",
        description="Generate C++, JSON and Unreal DataAssets from spreadsheet tables.",
    )
    parser.add_argument("--version", action="version", version=__version__)
    parser.add_argument(
        "--lang", choices=SUPPORTED,
        help="language of messages (default: $TABLEGEN_LANG or en)",
    )
    subparsers = parser.add_subparsers(dest="command", required=True)

    build = subparsers.add_parser("build", help="generate C++, client JSON and server JSON")
    build.add_argument("--input", required=True, type=Path, help="an .xlsx file or a folder of them")
    build.add_argument("--out-cpp", required=True, type=Path, help="output folder for C++ headers")
    build.add_argument("--out-client", required=True, type=Path, help="output folder for client JSON")
    build.add_argument("--out-server", required=True, type=Path, help="output folder for server JSON")
    build.add_argument("--prefix", default="Dt", help="C++ type prefix, e.g. Dt -> FDtItemsRow (default: Dt)")
    build.add_argument("--stamp", type=_iso8601, help="ISO 8601 value to record as generated_at in the manifest")
    build.add_argument(
        "--asset-base",
        default=DEFAULT_ASSET_BASE,
        help=f"base class of the table asset classes (default: {DEFAULT_ASSET_BASE})",
    )
    build.add_argument("--asset-base-header", help="header to include when --asset-base is changed")
    build.add_argument(
        "--ue-plugin",
        action="store_true",
        help=f"use the TableGen Unreal plugin: --asset-base {PLUGIN_ASSET_BASE} "
        f"--asset-base-header {PLUGIN_ASSET_BASE_HEADER} --runtime-header {PLUGIN_RUNTIME_HEADER}",
    )
    build.add_argument(
        "--runtime-header",
        help="header providing the TableGenRuntime lookup contract; enables row lookup "
        "functions, reference accessors and the registration header",
    )
    build.add_argument(
        "--asset-name",
        default=DEFAULT_ASSET_NAME,
        help="asset name pattern used for registration and baking; {table} is replaced "
        f"(default: {DEFAULT_ASSET_NAME})",
    )

    graph = subparsers.add_parser("graph", help="write the table reference graph as Mermaid Markdown")
    graph.add_argument("--input", required=True, type=Path)
    graph.add_argument("--out", required=True, type=Path)

    check = subparsers.add_parser(
        "check", help="check references in generated JSON (or validate a spreadsheet with --input)"
    )
    check.add_argument("--client", type=Path, help="client JSON folder written by build")
    check.add_argument("--server", type=Path, help="server JSON folder written by build")
    check.add_argument("--input", type=Path, help="validate a spreadsheet without writing anything")
    return parser


def main(argv: list[str] | None = None) -> int:
    parser = create_parser()
    args = parser.parse_args(argv)
    if args.lang:
        set_language(args.lang)
    if args.command == "check" and args.client is not None:
        try:
            failures = []
            for directory in (args.client, args.server):
                if directory is None:
                    continue
                found, warnings = check_directory(directory)
                for warning in warnings:
                    print(f"{directory}: {tr('경고', 'warning')}: {warning}", file=sys.stderr)
                failures.extend(f"{directory}: {failure}" for failure in found)
            for failure in failures:
                print(failure, file=sys.stderr)
            return 1 if failures else 0
        except CheckInputError as exc:
            print(exc, file=sys.stderr)
            return 2
    if args.command == "check" and (args.server is not None or args.input is None):
        parser.error(tr("check에는 --client 또는 --input이 필요합니다",
                        "check needs --client or --input"))
    try:
        model = load_model(args.input)
        if args.command == "check":
            return 0
        if args.command == "graph":
            emit_graph(model, args.out)
            return 0
        if not IDENTIFIER_RE.fullmatch(args.prefix):
            parser.error(tr("--prefix는 영문자로 시작하는 C++ 식별자여야 합니다",
                            "--prefix must be a C++ identifier starting with a letter"))
        outputs = (args.out_cpp, args.out_client, args.out_server)
        if len({path.resolve() for path in outputs}) != len(outputs):
            parser.error(tr("출력 디렉터리는 서로 달라야 합니다",
                            "the output folders must be different"))
        if args.ue_plugin:
            # Explicit options win; only unset ones take the plugin defaults.
            if args.asset_base == DEFAULT_ASSET_BASE:
                args.asset_base = PLUGIN_ASSET_BASE
            args.asset_base_header = args.asset_base_header or PLUGIN_ASSET_BASE_HEADER
            args.runtime_header = args.runtime_header or PLUGIN_RUNTIME_HEADER
        if args.asset_base != DEFAULT_ASSET_BASE and not args.asset_base_header:
            # Changing the base class without its header would emit code that does not compile.
            parser.error(tr("--asset-base를 바꾸면 --asset-base-header도 필요합니다",
                            "--asset-base requires --asset-base-header"))
        if "{table}" not in args.asset_name:
            # Every table would register under the same asset name.
            parser.error(tr("--asset-name에는 {table}이 들어가야 합니다",
                            "--asset-name must contain {table}"))
        if args.runtime_header:
            _check_member_names(model)
        for output in outputs:
            _clear_output(output)
        emit_cpp(
            model,
            args.out_cpp,
            args.prefix,
            args.asset_base,
            args.asset_base_header,
            args.runtime_header,
            args.asset_name,
        )
        emit_json(model, args.out_client, args.out_server, args.stamp, args.prefix, args.asset_name)
        return 0
    except ValidationErrors as exc:
        for message in exc.messages:
            print(message, file=sys.stderr)
        return 1


def _check_member_names(model) -> None:
    """Generated member functions must not clash with fields or with each other (C++ would not compile)."""
    errors = ErrorCollector()
    for table in model.tables:
        fields = {column.name for column in table.columns if column.scope in {"B", "C"}}
        seen: dict[str, str] = {}
        for name, cell in generated_member_names(table):
            if name in fields:
                errors.add(table.sheet, cell, tr(
                    f"생성할 함수 '{name}'이 같은 이름의 필드와 겹칩니다",
                    f"generated function '{name}' clashes with a field of the same name",
                ))
            elif name in seen:
                errors.add(table.sheet, cell, tr(
                    f"생성할 함수 '{name}'이 {seen[name]}에서 만든 함수와 겹칩니다",
                    f"generated function '{name}' clashes with the one generated for {seen[name]}",
                ))
            else:
                seen[name] = cell
    errors.raise_if_any()


def _clear_output(path: Path) -> None:
    if path.exists():
        if not path.is_dir():
            raise ValidationErrors([tr(
                f"출력!A1: 출력 경로가 디렉터리가 아닙니다: {path}",
                f"output!A1: the output path is not a folder: {path}",
            )])
        for child in path.iterdir():
            if child.is_dir():
                shutil.rmtree(child)
            else:
                child.unlink()
    path.mkdir(parents=True, exist_ok=True)


def _iso8601(value: str) -> str:
    try:
        datetime.fromisoformat(value)
    except ValueError as exc:
        raise argparse.ArgumentTypeError(tr("--stamp는 ISO 8601 형식이어야 합니다",
                                            "--stamp must be ISO 8601")) from exc
    return value


if __name__ == "__main__":
    raise SystemExit(main())
