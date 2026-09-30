"""Test harness: runs the `drtable` executable and writes schema workbooks for the tests.

The tests are black-box tests of the Rust executable. `main(argv)` runs it the way the
command line would and returns its exit code; its stdout and stderr are written to this
process's streams so pytest's capsys sees them. A usage error (exit code 2 with an
argparse-style "drtable: error:" line) raises SystemExit(2), like a command line parser.

The executable is `DRTABLE_BIN`, or `rust/target/release/drtable` built on first use.
"""

from __future__ import annotations

import io
import os
import re
import subprocess
import sys
import zipfile
from dataclasses import dataclass, field
from datetime import datetime
from pathlib import Path

from openpyxl import Workbook

ROOT = Path(__file__).resolve().parent.parent
SCHEMA_SUFFIXES = (".schema.xlsx", ".enum.xlsx")
IDENTIFIER_RE = re.compile(r"^[A-Za-z][A-Za-z0-9_]*$")
NAME_ROW, TYPE_ROW, SCOPE_ROW, DATA_ROW = 1, 2, 3, 4

_language = "ko"
_binary: str | None = None


def set_language(language: str) -> None:
    """Messages language for the executable (passed as DRTABLE_LANG)."""
    global _language
    _language = language


def language() -> str:
    return _language


def binary() -> str:
    """The executable under test, building it once when DRTABLE_BIN is not set."""
    global _binary
    if _binary is None:
        configured = os.environ.get("DRTABLE_BIN")
        if configured:
            _binary = configured
        else:
            subprocess.run(["cargo", "build", "--release", "--quiet"], cwd=ROOT / "rust", check=True)
            name = "drtable.exe" if os.name == "nt" else "drtable"
            _binary = str(ROOT / "rust" / "target" / "release" / name)
    return _binary


def main(argv: list[str] | None = None) -> int:
    """Runs `drtable argv...` and returns its exit code."""
    env = dict(os.environ, DRTABLE_LANG=_language)
    result = subprocess.run(
        [binary(), *(argv or [])], capture_output=True, text=True, encoding="utf-8", env=env, check=False,
    )
    sys.stdout.write(result.stdout)
    sys.stderr.write(result.stderr)
    if result.returncode == 2 and "drtable: error:" in result.stderr:
        raise SystemExit(2)
    return result.returncode


# --- Schema workbooks -------------------------------------------------------------------

# A definition row: (row, name, type or value, scope, comment).
Row = tuple[int, object, object, object, object]


@dataclass
class Schema:
    """A table (fields) or enum (values) definition to write as a schema workbook."""

    file: str
    name: str
    rows: list[Row] = field(default_factory=list)
    is_enum: bool = False


def render_xlsx(schema: Schema) -> bytes:
    """A schema workbook: table schema (Field, Type, Scope, Comment) or enum (Name, Value, Comment)."""
    workbook = Workbook()
    sheet = workbook.active
    sheet.title = schema.name
    if schema.is_enum:
        sheet.append(["Name", "Value", "Comment"])
        for _, name, value, _, comment in schema.rows:
            sheet.append([name, value, comment])
    else:
        sheet.append(["Field", "Type", "Scope", "Comment"])
        for _, name, type_, scope, comment in schema.rows:
            sheet.append([name, type_, scope, comment])
    fixed = datetime(2000, 1, 1)  # noqa: DTZ001 - openpyxl writes naive UTC times
    workbook.properties.created = fixed
    workbook.properties.modified = fixed
    buffer = io.BytesIO()
    workbook.save(buffer)
    # Pin zip entry times so the same definition always gives the same bytes.
    source = zipfile.ZipFile(io.BytesIO(buffer.getvalue()))
    output = io.BytesIO()
    with zipfile.ZipFile(output, "w", zipfile.ZIP_DEFLATED) as target:
        for info in source.infolist():
            pinned = zipfile.ZipInfo(info.filename, date_time=(2000, 1, 1, 0, 0, 0))
            pinned.compress_type = zipfile.ZIP_DEFLATED
            target.writestr(pinned, source.read(info.filename))
    return output.getvalue()


# --- Files and sheet names (as the executable reads them) --------------------------------

def strip_sheet_comment(sheet_title: str) -> str:
    return sheet_title.split("#", 1)[0].strip()


def table_name_of(sheet_title: str) -> str | None:
    if sheet_title.startswith("#"):
        return None
    return strip_sheet_comment(sheet_title)


def find_files(input_path: Path, suffixes: tuple[str, ...] = (".xlsx",)) -> list[tuple[Path, str]]:
    """Files with one of the suffixes under input_path (or input_path itself), sorted by relative path."""
    if input_path.is_file():
        return [(input_path, input_path.name)] if input_path.name.lower().endswith(suffixes) else []
    found = []
    for path in input_path.rglob("*"):
        relative = path.relative_to(input_path)
        if (
            path.is_file()
            and path.name.lower().endswith(suffixes)
            and not path.name.startswith("~$")
            and not any(part.startswith(".") for part in relative.parts)
        ):
            found.append((path, relative.as_posix()))
    return sorted(found, key=lambda item: item[1])
