"""Schema files: where tables and enums are defined.

Tables and enums are defined apart from the data, so editing data never changes generated code.

* Table schemas live in the schema folder: ``<Table>.schema.xlsx``.
* Enum schemas live in the enum folder (default ``<schema folder>/Enums``): ``<Enum>.enum.xlsx``.
  An enum's values are part of its schema because they become a C++ UENUM.

Table schema: one sheet named after the table; row 1 holds labels, from row 2 each row is a
field (A name, B type, C scope, D comment). Enum schema: one sheet named after the enum; from
row 2 each row is an enumerator (A name, B value, C comment).
"""

from __future__ import annotations

import io
import zipfile
from dataclasses import dataclass, field
from datetime import datetime
from pathlib import Path
from xml.etree.ElementTree import ParseError
from zipfile import BadZipFile

from openpyxl import Workbook, load_workbook
from openpyxl.utils.exceptions import InvalidFileException

from .errors import ErrorCollector
from .i18n import tr
from .schema import IDENTIFIER_RE
from .sources import find_files

TABLE_SUFFIXES = (".schema.xlsx",)
ENUM_SUFFIXES = (".enum.xlsx",)
SCHEMA_SUFFIXES = TABLE_SUFFIXES + ENUM_SUFFIXES
TABLE_LABELS = ("Field", "Type", "Scope", "Comment")
ENUM_LABELS = ("Name", "Value", "Comment")
DEFAULT_ENUM_FOLDER = "Enums"

# A definition row: (row, value, value, value, comment)
#   table field: (row, name, type, scope, comment)
#   enumerator:  (row, name, value, None, comment)
Row = tuple[int, object, object, object, object]


@dataclass
class Schema:
    """The definition of one table (its fields) or one enum (its values)."""

    file: str
    name: str
    rows: list[Row]
    is_enum: bool = False
    title: str = ""  # sheet name as written

    @property
    def where(self) -> str:
        return f"[{self.file}]{self.title or self.name}"

    def cell(self, row: int, column: int) -> str:
        """The cell of a definition (column 1 = A)."""
        return f"{'ABCD'[column - 1]}{row}"

    def at(self, row: int, column: int = 1) -> str:
        return f"{self.where}!{self.cell(row, column)}"

    def raw_columns(self) -> list[tuple[int, object, object, object]]:
        """Table fields as (row, name, type, scope)."""
        return [(row, name, type_, scope) for row, name, type_, scope, _ in self.rows]

    def enumerators(self) -> list[tuple[int, object, object, object]]:
        """Enum values as (row, name, value, comment)."""
        return [(row, name, value, comment) for row, name, value, _, comment in self.rows]

@dataclass
class Schemas:
    tables: dict[str, Schema] = field(default_factory=dict)
    enums: dict[str, Schema] = field(default_factory=dict)

    def all(self) -> list[Schema]:
        return [*self.tables.values(), *self.enums.values()]


def enum_folder(schema_root: Path, enum_root: Path | None) -> Path:
    if enum_root is not None:
        return enum_root
    base = schema_root.parent if schema_root.is_file() else schema_root
    return base / DEFAULT_ENUM_FOLDER


def load_schemas(schema_root: Path, enum_root: Path, errors: ErrorCollector) -> Schemas:
    """Table schemas under schema_root (outside the enum folder) and enum schemas under enum_root."""
    result = Schemas()
    enum_resolved = enum_root.resolve()
    table_files = [
        item for item in find_files(schema_root, TABLE_SUFFIXES, allow_empty=True)
        if not _inside(item[0], enum_resolved)
    ]
    for path, relative in find_files(schema_root, ENUM_SUFFIXES, allow_empty=True):
        if not _inside(path, enum_resolved):
            errors.add(f"[{relative}]", "A1", tr(
                f"열거형 스키마는 열거형 폴더({enum_root})에 두어야 합니다",
                f"enum schemas belong in the enum folder ({enum_root})",
            ))
    enum_files = []
    if enum_root.exists():
        base = schema_root.parent if schema_root.is_file() else schema_root
        for path, _ in find_files(enum_root, SCHEMA_SUFFIXES, allow_empty=True):
            relative = _relative(path, base)
            if path.name.lower().endswith(TABLE_SUFFIXES):
                errors.add(f"[{relative}]", "A1", tr(
                    "열거형 폴더에는 열거형 스키마(.enum.xlsx)만 둡니다",
                    "the enum folder holds enum schemas (.enum.xlsx) only",
                ))
            else:
                enum_files.append((path, relative))
    _collect(table_files, TABLE_SUFFIXES, False, result.tables, errors)
    _collect(enum_files, ENUM_SUFFIXES, True, result.enums, errors)
    return result


def render_xlsx(schema: Schema) -> bytes:
    """A schema workbook as bytes. The same definition always gives the same bytes."""
    workbook = Workbook()
    sheet = workbook.active
    sheet.title = schema.name
    if schema.is_enum:
        sheet.append(list(ENUM_LABELS))
        for _, name, value, _, comment in schema.rows:
            sheet.append([name, value, comment])
        widths = (28, 10, 40)
    else:
        sheet.append(list(TABLE_LABELS))
        for _, name, type_, scope, comment in schema.rows:
            sheet.append([name, type_, scope, comment])
        widths = (24, 28, 10, 40)
    for index, width in enumerate(widths):
        sheet.column_dimensions["ABCD"[index]].width = width
    fixed = datetime(2000, 1, 1)  # noqa: DTZ001 - openpyxl writes naive UTC times
    workbook.properties.created = fixed
    workbook.properties.modified = fixed
    workbook.properties.lastModifiedBy = None
    workbook.properties.creator = "DrTableSystem"
    buffer = io.BytesIO()
    workbook.save(buffer)
    # Zip entries carry the time they were written; pin it so the bytes are reproducible.
    source = zipfile.ZipFile(io.BytesIO(buffer.getvalue()))
    output = io.BytesIO()
    with zipfile.ZipFile(output, "w", zipfile.ZIP_DEFLATED) as target:
        for info in source.infolist():
            pinned = zipfile.ZipInfo(info.filename, date_time=(2000, 1, 1, 0, 0, 0))
            pinned.compress_type = zipfile.ZIP_DEFLATED
            pinned.external_attr = info.external_attr
            target.writestr(pinned, source.read(info.filename))
    return output.getvalue()


def _inside(path: Path, folder: Path) -> bool:
    try:
        path.resolve().relative_to(folder)
    except ValueError:
        return False
    return True


def _relative(path: Path, base: Path) -> str:
    try:
        return path.resolve().relative_to(base.resolve()).as_posix()
    except ValueError:
        return path.name


def _suffix(path: Path, suffixes: tuple[str, ...]) -> str:
    return next(suffix for suffix in suffixes if path.name.lower().endswith(suffix))


def _collect(
    files: list[tuple[Path, str]],
    suffixes: tuple[str, ...],
    is_enum: bool,
    into: dict[str, Schema],
    errors: ErrorCollector,
) -> None:
    for path, relative in files:
        schema = _read_xlsx(path, relative, is_enum, errors)
        if schema is None:
            continue
        suffix = _suffix(path, suffixes)
        stem = path.name[: -len(suffix)]
        if schema.name != stem:
            errors.add(schema.where, "A1", tr(
                f"이름 '{schema.name}'과 파일 이름 '{path.name}'이 다릅니다. 파일 이름은 '{schema.name}{suffix}'여야 합니다",
                f"name '{schema.name}' does not match the file name '{path.name}'; name the file '{schema.name}{suffix}'",
            ))
            continue
        if schema.name in into:
            errors.add(schema.where, "A1", tr(
                f"'{schema.name}'의 스키마가 {into[schema.name].where}에도 있습니다",
                f"'{schema.name}' also has a schema in {into[schema.name].where}",
            ))
            continue
        into[schema.name] = schema


def _read_xlsx(path: Path, relative: str, is_enum: bool, errors: ErrorCollector) -> Schema | None:
    try:
        workbook = load_workbook(path, data_only=True, read_only=True)
    except (BadZipFile, InvalidFileException, KeyError, OSError, ParseError, ValueError) as exc:
        errors.add(f"[{relative}]", "A1", tr(f"xlsx 파일을 읽을 수 없습니다: {exc}", f"cannot read the xlsx file: {exc}"))
        return None
    try:
        sheets = [sheet for sheet in workbook.worksheets if not sheet.title.startswith("#")]
        if len(sheets) != 1:
            errors.add(f"[{relative}]", "A1", tr(
                "스키마 파일에는 정의 시트가 하나만 있어야 합니다('#' 메모 시트 제외)",
                "a schema file needs exactly one definition sheet ('#' note sheets aside)",
            ))
            return None
        sheet = sheets[0]
        name = sheet.title.split("#", 1)[0].strip()
        schema = Schema(relative, name, [], is_enum=is_enum, title=sheet.title)
        if not _valid_name(schema, errors):
            return None
        width = 3 if is_enum else 4
        for row, values in enumerate(sheet.iter_rows(min_row=2, max_col=width, values_only=True), start=2):
            values = tuple(values) + (None,) * (width - len(values))
            if all(value in (None, "") for value in values[: width - 1]):
                continue
            if values[0] in (None, ""):
                errors.add(schema.where, f"A{row}", tr("이름이 비어 있습니다", "the name is empty"))
                continue
            if is_enum:
                schema.rows.append((row, values[0], values[1], None, values[2]))
            else:
                schema.rows.append((row, *values))
        return schema
    finally:
        workbook.close()


def _valid_name(schema: Schema, errors: ErrorCollector) -> bool:
    if IDENTIFIER_RE.fullmatch(schema.name):
        return True
    errors.add(schema.where, "A1", tr(
        f"올바르지 않은 이름 '{schema.name}'", f"invalid name '{schema.name}'",
    ))
    return False
