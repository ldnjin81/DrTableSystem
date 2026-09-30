"""tablegen 명령줄 인터페이스."""

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
from .schema import IDENTIFIER_RE


def create_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="tablegen")
    parser.add_argument("--version", action="version", version=__version__)
    subparsers = parser.add_subparsers(dest="command", required=True)

    build = subparsers.add_parser("build", help="xlsx에서 산출물을 생성합니다")
    build.add_argument("--input", required=True, type=Path)
    build.add_argument("--out-cpp", required=True, type=Path)
    build.add_argument("--out-client", required=True, type=Path)
    build.add_argument("--out-server", required=True, type=Path)
    build.add_argument("--prefix", default="Dt")
    build.add_argument("--stamp", type=_iso8601, help="매니페스트에 넣을 ISO 8601 식별자")
    build.add_argument(
        "--asset-base",
        default=DEFAULT_ASSET_BASE,
        help="테이블 에셋 클래스의 기반 클래스(기본 UPrimaryDataAsset)",
    )
    build.add_argument(
        "--asset-base-header",
        help="기반 클래스를 바꿀 때 include할 헤더 경로",
    )
    build.add_argument(
        "--runtime-header",
        help="TableGenRuntime 조회 계약을 제공하는 헤더. 주면 행 조회·참조 함수와 등록 헤더를 생성",
    )
    build.add_argument(
        "--asset-name",
        default=DEFAULT_ASSET_NAME,
        help="등록에 쓸 에셋 이름 형식. {table}이 테이블 이름으로 바뀐다(기본 DA_{table})",
    )

    graph = subparsers.add_parser("graph", help="Mermaid 참조 그래프를 생성합니다")
    graph.add_argument("--input", required=True, type=Path)
    graph.add_argument("--out", required=True, type=Path)

    check = subparsers.add_parser("check", help="생성된 JSON의 참조를 검사합니다")
    check.add_argument("--client", type=Path)
    check.add_argument("--server", type=Path)
    check.add_argument("--input", type=Path, help="기존 엑셀 스키마 검사")
    return parser


def main(argv: list[str] | None = None) -> int:
    parser = create_parser()
    args = parser.parse_args(argv)
    if args.command == "check" and args.client is not None:
        try:
            failures = []
            for directory in (args.client, args.server):
                if directory is None:
                    continue
                found, warnings = check_directory(directory)
                for warning in warnings:
                    print(f"{directory}: 경고: {warning}", file=sys.stderr)
                failures.extend(f"{directory}: {failure}" for failure in found)
            for failure in failures:
                print(failure, file=sys.stderr)
            return 1 if failures else 0
        except CheckInputError as exc:
            print(exc, file=sys.stderr)
            return 2
    if args.command == "check" and (args.server is not None or args.input is None):
        parser.error("check에는 --client 또는 --input이 필요합니다")
    try:
        model = load_model(args.input)
        if args.command == "check":
            return 0
        if args.command == "graph":
            emit_graph(model, args.out)
            return 0
        if not IDENTIFIER_RE.fullmatch(args.prefix):
            parser.error("--prefix는 영문자로 시작하는 C++ 식별자여야 합니다")
        outputs = (args.out_cpp, args.out_client, args.out_server)
        if len({path.resolve() for path in outputs}) != len(outputs):
            parser.error("출력 디렉터리는 서로 달라야 합니다")
        if args.asset_base != DEFAULT_ASSET_BASE and not args.asset_base_header:
            # 헤더 없이 기반 클래스만 바꾸면 컴파일되지 않는 코드가 나온다.
            # 조용히 내보내는 대신 생성 단계에서 멈춘다.
            parser.error("--asset-base를 바꾸면 --asset-base-header도 필요합니다")
        if "{table}" not in args.asset_name:
            # 모든 테이블이 같은 에셋 이름으로 등록되면 조회가 뒤섞인다.
            parser.error("--asset-name에는 {table}이 들어가야 합니다")
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
    """생성할 함수 이름이 필드명이나 다른 생성 함수와 겹치면 C++가 컴파일되지 않는다."""
    errors = ErrorCollector()
    for table in model.tables:
        fields = {column.name for column in table.columns if column.scope in {"B", "C"}}
        seen: dict[str, str] = {}
        for name, cell in generated_member_names(table):
            if name in fields:
                errors.add(table.sheet, cell, f"생성할 함수 '{name}'이 같은 이름의 필드와 겹칩니다")
            elif name in seen:
                errors.add(
                    table.sheet, cell,
                    f"생성할 함수 '{name}'이 {seen[name]}에서 만든 함수와 겹칩니다",
                )
            else:
                seen[name] = cell
    errors.raise_if_any()


def _clear_output(path: Path) -> None:
    if path.exists():
        if not path.is_dir():
            raise ValidationErrors([f"출력!A1: 출력 경로가 디렉터리가 아닙니다: {path}"])
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
        raise argparse.ArgumentTypeError("--stamp는 ISO 8601 형식이어야 합니다") from exc
    return value


if __name__ == "__main__":
    raise SystemExit(main())
