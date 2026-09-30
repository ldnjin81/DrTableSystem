"""The test suite asserts the Korean messages; English is covered by test_i18n.py.

Most tests describe a table compactly, as one sheet with the field name, type and scope in
rows 1-3 (enums in <enum> sheets). Saving such a workbook also writes its schema files next to
it (tables) and under Enums/ (enums), see legacy_layout.py, so each test keeps its fixture in
one readable place. Tests that build schema files themselves save with ``plain_save``.
"""

from __future__ import annotations

import contextlib
import io
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

import pytest
from legacy_layout import extract_schemas
from openpyxl import Workbook, load_workbook

import drtable.cli
from drtable.i18n import language, set_language
from drtable.schemafile import SCHEMA_SUFFIXES

plain_save = Workbook.save


_extracting = False


def _save_with_schemas(self: Workbook, filename) -> None:
    global _extracting
    plain_save(self, filename)
    if _extracting or not isinstance(filename, str | Path):
        return
    path = Path(filename)
    if (
        path.suffix.lower() == ".xlsx"
        and not path.name.lower().endswith(SCHEMA_SUFFIXES)
        and _has_old_headers(self)
    ):
        _extracting = True
        try:
            extract_schemas(path, path.parent, path.parent / "Enums", overwrite=True)
            # Enum values now live in the enum schemas, so the data workbook drops its
            # <enum> sheets (the test's own workbook object is left as it is).
            if any(sheet.title.startswith("<enum>") for sheet in self.worksheets):
                saved = load_workbook(path)
                for sheet in list(saved.worksheets):
                    if sheet.title.startswith("<enum>"):
                        saved.remove(sheet)
                if saved.worksheets:
                    plain_save(saved, path)
                else:
                    path.unlink()
        finally:
            _extracting = False


def _has_old_headers(workbook: Workbook) -> bool:
    """Old layout: some sheet declares a key type such as ID<int32> in row 2."""
    return any(
        str(cell.value).strip().lower().startswith("id<")
        for sheet in workbook.worksheets
        for cell in sheet[2]
        if cell.value is not None
    )


@pytest.fixture(autouse=True)
def _korean_messages():
    set_language("ko")
    yield
    set_language("ko")


@pytest.fixture(autouse=True)
def _schemas_from_old_headers(monkeypatch: pytest.MonkeyPatch):
    monkeypatch.setattr(Workbook, "save", _save_with_schemas)
    yield


# Parity mode: with DRTABLE_BIN set to the Rust binary, every main() call in the tests runs the
# Rust implementation. The Python implementation runs first on the same arguments (outputs
# redirected to a temporary folder); exit code, stdout, stderr and every written file must be
# identical, so the whole suite checks the port byte for byte.
_python_main = drtable.cli.main
_OUTPUT_OPTIONS = ("--out-cpp", "--out-client", "--out-server", "--out")


def _snapshot(folder: Path) -> dict[str, bytes]:
    if folder.is_file():
        return {"": folder.read_bytes()}
    if not folder.is_dir():
        return {}
    return {p.relative_to(folder).as_posix(): p.read_bytes() for p in sorted(folder.rglob("*")) if p.is_file()}


def _rust_main(argv: list[str] | None = None) -> int:
    argv = list(argv or [])
    lang = language()
    command = next((a for a in argv if not a.startswith("-") and a not in ("en", "ko")), "")
    compare_outputs = command in ("build", "graph")
    reference: dict[str, dict[str, bytes]] = {}
    scratch = Path(tempfile.mkdtemp(prefix="drtable-ref-"))
    python_argv = list(argv)
    real_outputs: dict[str, Path] = {}
    if compare_outputs:
        for index, arg in enumerate(argv[:-1]):
            if arg in _OUTPUT_OPTIONS:
                real_outputs[arg] = Path(argv[index + 1])
                python_argv[index + 1] = str(scratch / arg.strip("-"))
    out, err = io.StringIO(), io.StringIO()
    python_exit: object
    if command == "new":
        # These only create files; the Python run would create them first. The tests check
        # the Rust output themselves.
        env = dict(os.environ, DRTABLE_LANG=lang)
        result = subprocess.run([os.environ["DRTABLE_BIN"], *argv], capture_output=True, text=True, encoding="utf-8", env=env, check=False)
        sys.stdout.write(result.stdout)
        sys.stderr.write(result.stderr)
        if result.returncode == 2 and "drtable: error:" in result.stderr:
            raise SystemExit(2)
        return result.returncode
    try:
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            python_exit = _python_main(python_argv)
    except SystemExit as exc:
        python_exit = ("SystemExit", exc.code)
    set_language(lang)
    if compare_outputs:
        for option in real_outputs:
            reference[option] = _snapshot(scratch / option.strip("-"))
    shutil.rmtree(scratch, ignore_errors=True)

    env = dict(os.environ, DRTABLE_LANG=lang)
    result = subprocess.run([os.environ["DRTABLE_BIN"], *argv], capture_output=True, text=True, encoding="utf-8", env=env, check=False)
    sys.stdout.write(result.stdout)
    sys.stderr.write(result.stderr)
    rust_exit: object = result.returncode
    if result.returncode == 2 and "drtable: error:" in result.stderr:
        rust_exit = ("SystemExit", 2)

    problems = []
    if rust_exit != python_exit:
        problems.append(f"exit: rust {rust_exit!r} != python {python_exit!r}")
    usage_error = isinstance(python_exit, tuple)
    if not usage_error and result.stderr != err.getvalue():
        problems.append(f"stderr differs:\n--- rust\n{result.stderr}--- python\n{err.getvalue()}")
    if not usage_error and command != "new" and result.stdout != out.getvalue():
        problems.append(f"stdout differs:\n--- rust\n{result.stdout}--- python\n{out.getvalue()}")
    for option, path in real_outputs.items():
        actual = _snapshot(path)
        if actual != reference[option]:
            names = sorted(set(actual) | set(reference[option]))
            diff = [n for n in names if actual.get(n) != reference[option].get(n)]
            problems.append(f"{option} differs in {diff}")
    if problems:
        raise AssertionError("Rust/Python parity: " + "\n".join(problems))
    if isinstance(rust_exit, tuple):
        raise SystemExit(2)
    return rust_exit


if os.environ.get("DRTABLE_BIN"):
    drtable.cli.main = _rust_main
