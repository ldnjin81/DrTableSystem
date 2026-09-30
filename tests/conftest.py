"""Black-box tests of the drtable executable (see harness.py). They assert the Korean
messages; English is covered by test_i18n.py.

Most tests describe a table compactly, as one sheet with the field name, type and scope in
rows 1-3 (enums in <enum> sheets). Saving such a workbook also writes its schema files next to
it (tables) and under Enums/ (enums), see legacy_layout.py, so each test keeps its fixture in
one readable place. Tests that build schema files themselves save with ``plain_save``.
"""

from __future__ import annotations

from pathlib import Path

import pytest
from harness import SCHEMA_SUFFIXES, set_language
from legacy_layout import extract_schemas
from openpyxl import Workbook, load_workbook

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
