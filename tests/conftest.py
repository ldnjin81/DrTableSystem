"""The test suite asserts the Korean messages; English is covered by test_i18n.py.

Most tests describe a table the old way, as one sheet with the field name, type and scope in
rows 1-3. Saving such a workbook also writes its <Name>.schema.xlsx files next to it (the same
conversion `drtable migrate` performs), so each test keeps its fixture in one readable place.
Tests that build schema files themselves save with ``plain_save``.
"""

from __future__ import annotations

from pathlib import Path

import pytest
from openpyxl import Workbook

from drtable.schemafile import SCHEMA_SUFFIXES
from drtable.i18n import set_language
from drtable.migrate import extract_schemas

plain_save = Workbook.save


def _save_with_schemas(self: Workbook, filename) -> None:
    plain_save(self, filename)
    if not isinstance(filename, str | Path):
        return
    path = Path(filename)
    if path.suffix.lower() == ".xlsx" and not path.name.lower().endswith(SCHEMA_SUFFIXES):
        extract_schemas(path, path.parent, overwrite=True)


@pytest.fixture(autouse=True)
def _korean_messages():
    set_language("ko")
    yield
    set_language("ko")


@pytest.fixture(autouse=True)
def _schemas_from_old_headers(monkeypatch: pytest.MonkeyPatch):
    monkeypatch.setattr(Workbook, "save", _save_with_schemas)
    yield
