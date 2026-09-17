"""tablegen 명령줄 인터페이스."""

from __future__ import annotations

import argparse
import shutil
import sys
from datetime import datetime
from pathlib import Path

from . import __version__
from .emit_cpp import DEFAULT_ASSET_BASE, emit_cpp
from .emit_json import emit_json
from .errors import ValidationErrors
from .excel import load_model
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

    check = subparsers.add_parser("check", help="파일을 쓰지 않고 xlsx를 검증합니다")
    check.add_argument("--input", required=True, type=Path)
    return parser


def main(argv: list[str] | None = None) -> int:
    parser = create_parser()
    args = parser.parse_args(argv)
    try:
        model = load_model(args.input)
        if args.command == "check":
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
        for output in outputs:
            _clear_output(output)
        emit_cpp(model, args.out_cpp, args.prefix, args.asset_base, args.asset_base_header)
        emit_json(model, args.out_client, args.out_server, args.stamp)
        return 0
    except ValidationErrors as exc:
        for message in exc.messages:
            print(message, file=sys.stderr)
        return 1


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
