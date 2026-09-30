"""Moves the field definitions of old workbooks (three header rows) into schema files."""

from __future__ import annotations

from pathlib import Path

from openpyxl import load_workbook

from .schema import IDENTIFIER_RE, NAME_ROW, SCOPE_ROW
from .schemafile import SCHEMA_SUFFIXES, XLSX_SUFFIX, dump_yaml, render_xlsx
from .sources import ENUM_SHEET_RE, find_files, strip_sheet_comment, table_name_of


def extract_schemas(
    input_path: Path, schema_dir: Path, overwrite: bool = False, fmt: str = "xlsx",
) -> list[Path]:
    """Writes a schema file for every table and enum sheet found under input_path.

    ``fmt`` is "xlsx" (<Name>.schema.xlsx is the source) or "yaml" (<Name>.schema.yaml is the
    source; run schema-export for the xlsx view). The first sheet of a table, in file and
    sheet order, defines its schema. Existing schema files are kept unless ``overwrite`` is
    set. Data workbooks are not modified. Returns the files written.
    """
    written: list[Path] = []
    seen: set[str] = set()
    for path, _ in find_files(input_path):
        if path.name.lower().endswith(SCHEMA_SUFFIXES):
            continue
        workbook = load_workbook(path, read_only=True)
        try:
            for sheet in workbook.worksheets:
                title = strip_sheet_comment(sheet.title)
                match = ENUM_SHEET_RE.fullmatch(title)
                name = match.group("name") if match else table_name_of(sheet.title)
                if not name or not IDENTIFIER_RE.fullmatch(name) or name in seen:
                    continue
                seen.add(name)
                target = schema_dir / (f"{name}.schema.yaml" if fmt == "yaml" else f"{name}{XLSX_SUFFIX}")
                if target.exists() and not overwrite:
                    continue
                fields = _header_fields(sheet)
                if not fields:
                    continue
                target.parent.mkdir(parents=True, exist_ok=True)
                sheet_title = f"<enum>{name}" if match else name
                if fmt == "yaml":
                    target.write_text(dump_yaml(sheet_title, fields), encoding="utf-8")
                else:
                    target.write_bytes(render_xlsx(sheet_title, fields))
                written.append(target)
        finally:
            workbook.close()
    return written


def _header_fields(sheet: object) -> list[tuple[int, object, object, object, object]]:
    rows = list(sheet.iter_rows(min_row=NAME_ROW, max_row=SCOPE_ROW, values_only=True))
    rows += [()] * (SCOPE_ROW - len(rows))
    names, types, scopes = (list(row) for row in rows[:3])
    fields = []
    for index, name in enumerate(names):
        if name in (None, ""):
            break
        type_value = types[index] if index < len(types) else None
        scope_value = scopes[index] if index < len(scopes) else None
        fields.append((index + 2, name, type_value, scope_value, None))
    return fields
