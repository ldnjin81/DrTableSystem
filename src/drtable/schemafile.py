"""Schema files: where tables and enums are defined.

Tables and enums are defined apart from the data, so editing data never changes generated code.

* Table schemas live in the schema folder: ``<Table>.schema.xlsx`` or ``<Table>.schema.yaml``.
* Enum schemas live in the enum folder (default ``<schema folder>/Enums``): ``<Enum>.enum.xlsx``
  or ``<Enum>.enum.yaml``. An enum's values are part of its schema because they become a
  C++ UENUM.

When a YAML file and an xlsx file of the same name exist, the YAML file is the source and the
xlsx file is a view generated from it (``drtable schema-export``), which data workbooks can show
through formulas.

Table xlsx: one sheet named after the table; row 1 holds labels, from row 2 each row is a field
(A name, B type, C scope, D comment). Enum xlsx: one sheet named after the enum; from row 2 each
row is an enumerator (A name, B value, C comment). YAML::

    table: Items                    enum: ItemType
    fields:                         values:
      - name: Id                      - name: Weapon
        type: ID<int32>                 value: 0        # optional: previous + 1
        scope: all                      comment: swords and bows
        comment: row key
"""

from __future__ import annotations

import io
import zipfile
from dataclasses import dataclass, field
from datetime import datetime
from pathlib import Path
from xml.etree.ElementTree import ParseError
from zipfile import BadZipFile

import yaml
from openpyxl import Workbook, load_workbook
from openpyxl.utils.exceptions import InvalidFileException

from .errors import ErrorCollector
from .i18n import tr
from .schema import IDENTIFIER_RE
from .sources import find_files

TABLE_SUFFIXES = (".schema.xlsx", ".schema.yaml", ".schema.yml")
ENUM_SUFFIXES = (".enum.xlsx", ".enum.yaml", ".enum.yml")
SCHEMA_SUFFIXES = TABLE_SUFFIXES + ENUM_SUFFIXES
YAML_SUFFIXES = (".yaml", ".yml")
TABLE_LABELS = ("Field", "Type", "Scope", "Comment")
ENUM_LABELS = ("Name", "Value", "Comment")
DEFAULT_ENUM_FOLDER = "Enums"

# A definition row: (row or line, value, value, value, comment)
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
    kind: str = "xlsx"  # "xlsx" or "yaml"
    title: str = ""  # xlsx sheet name as written
    view: Schema | None = field(default=None, repr=False)  # the xlsx view of a YAML schema
    # YAML: (definition line, column 1/2/3) -> line of that key.
    lines: dict[tuple[int, int], int] = field(default_factory=dict, repr=False)

    @property
    def where(self) -> str:
        return self.file if self.kind == "yaml" else f"[{self.file}]{self.title or self.name}"

    def cell(self, row: int, column: int) -> str:
        """xlsx: the cell (column 1 = A); YAML: the line of that key."""
        if self.kind == "yaml":
            return str(self.lines.get((row, column), row))
        return f"{'ABCD'[column - 1]}{row}"

    def at(self, row: int, column: int = 1) -> str:
        separator = ":" if self.kind == "yaml" else "!"
        return f"{self.where}{separator}{self.cell(row, column)}"

    def raw_columns(self) -> list[tuple[int, object, object, object]]:
        """Table fields as (row, name, type, scope)."""
        return [(row, name, type_, scope) for row, name, type_, scope, _ in self.rows]

    def enumerators(self) -> list[tuple[int, object, object, object]]:
        """Enum values as (row, name, value, comment)."""
        return [(row, name, value, comment) for row, name, value, _, comment in self.rows]

    def definition(self) -> list[tuple[str, ...]]:
        """Values only, for comparing a YAML source with its xlsx view."""
        return [
            tuple("" if value is None else str(value).strip() for value in item[1:])
            for item in self.rows
        ]


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
                    "열거형 폴더에는 열거형 스키마(.enum.xlsx/.enum.yaml)만 둡니다",
                    "the enum folder holds enum schemas (.enum.xlsx/.enum.yaml) only",
                ))
            else:
                enum_files.append((path, relative))
    _collect(table_files, TABLE_SUFFIXES, False, result.tables, errors)
    _collect(enum_files, ENUM_SUFFIXES, True, result.enums, errors)
    return result


def stale_views(schemas: Schemas) -> list[str]:
    """Warnings for xlsx views that no longer match their YAML source."""
    return [
        tr(
            f"{schema.view.where}: {schema.file}와 내용이 다릅니다. drtable schema-export로 다시 만드세요",
            f"{schema.view.where}: does not match {schema.file}; regenerate it with drtable schema-export",
        )
        for schema in schemas.all()
        if schema.kind == "yaml" and schema.view is not None
        and schema.view.definition() != schema.definition()
    ]


def export_views(root: Path, check: bool = False) -> tuple[list[Path], list[str]]:
    """Writes the xlsx view next to every YAML schema under root (tables and enums).

    Output is deterministic and unchanged files are not rewritten. With ``check`` nothing is
    written; the files that would change are returned instead (for CI).
    Returns (changed files, errors).
    """
    errors = ErrorCollector()
    changed: list[Path] = []
    yaml_suffixes = tuple(suffix for suffix in SCHEMA_SUFFIXES if suffix.endswith(YAML_SUFFIXES))
    for path, relative in find_files(root, yaml_suffixes, allow_empty=True):
        is_enum = path.name.lower().endswith(ENUM_SUFFIXES)
        schema = _read_yaml(path, relative, is_enum, errors)
        if schema is None:
            continue
        suffix = _suffix(path, SCHEMA_SUFFIXES)
        target = path.with_name(path.name[: -len(suffix)] + (".enum.xlsx" if is_enum else ".schema.xlsx"))
        data = render_xlsx(schema)
        if target.exists() and target.read_bytes() == data:
            continue
        changed.append(target)
        if not check:
            target.write_bytes(data)
    return changed, errors.messages


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


def dump_yaml(schema: Schema) -> str:
    """A schema as YAML text (for the YAML source layout)."""
    if schema.is_enum:
        lines = [f"enum: {schema.name}", "values:"]
        for _, name, value, _, comment in schema.rows:
            lines.append(f"  - name: {_scalar(name)}")
            if value not in (None, ""):
                lines.append(f"    value: {_scalar(value)}")
            if comment not in (None, ""):
                lines.append(f"    comment: {_scalar(comment)}")
    else:
        lines = [f"table: {schema.name}", "fields:"]
        for _, name, type_, scope, comment in schema.rows:
            lines.append(f"  - name: {_scalar(name)}")
            lines.append(f"    type: {_scalar(type_)}")
            lines.append(f"    scope: {_scalar(scope)}")
            if comment not in (None, ""):
                lines.append(f"    comment: {_scalar(comment)}")
    return "\n".join(lines) + "\n"


def _scalar(value: object) -> str:
    if isinstance(value, int) and not isinstance(value, bool):
        return str(value)
    text = "" if value is None else str(value)
    dumped = yaml.safe_dump(text, allow_unicode=True, default_flow_style=True, width=1 << 16)
    return dumped.removesuffix("\n...\n").removesuffix("\n")


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
    views: dict[str, Schema] = {}
    for path, relative in files:
        is_yaml = path.name.lower().endswith(YAML_SUFFIXES)
        if is_yaml:
            schema = _read_yaml(path, relative, is_enum, errors)
        else:
            schema = _read_xlsx(path, relative, is_enum, errors)
        if schema is None:
            continue
        suffix = _suffix(path, suffixes)
        stem = path.name[: -len(suffix)]
        if schema.name != stem:
            errors.add(schema.where, "1" if is_yaml else "A1", tr(
                f"이름 '{schema.name}'과 파일 이름 '{path.name}'이 다릅니다. 파일 이름은 '{schema.name}{suffix}'여야 합니다",
                f"name '{schema.name}' does not match the file name '{path.name}'; name the file '{schema.name}{suffix}'",
            ))
            continue
        bucket = into if is_yaml else views
        if schema.name in bucket:
            errors.add(schema.where, "1" if is_yaml else "A1", tr(
                f"'{schema.name}'의 스키마가 {bucket[schema.name].where}에도 있습니다",
                f"'{schema.name}' also has a schema in {bucket[schema.name].where}",
            ))
            continue
        bucket[schema.name] = schema
    for name, view in views.items():
        if name in into:
            into[name].view = view
        else:
            into[name] = view


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


def _read_yaml(path: Path, relative: str, is_enum: bool, errors: ErrorCollector) -> Schema | None:
    try:
        root = yaml.compose(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError) as exc:
        errors.add(relative, "1", tr(f"파일을 읽을 수 없습니다: {exc}", f"cannot read the file: {exc}"))
        return None
    except yaml.YAMLError as exc:
        line = getattr(getattr(exc, "problem_mark", None), "line", 0) + 1
        errors.add(relative, str(line), tr(f"YAML 문법 오류: {exc}", f"YAML syntax error: {exc}"))
        return None
    kind, list_key = ("enum", "values") if is_enum else ("table", "fields")
    if not isinstance(root, yaml.MappingNode):
        errors.add(relative, "1", tr(f"'{kind}:'와 '{list_key}:'가 있는 매핑이어야 합니다", f"expected a mapping with '{kind}:' and '{list_key}:'"))
        return None
    top = {key.value: value for key, value in root.value}
    if not isinstance(top.get(kind), yaml.ScalarNode):
        errors.add(relative, "1", tr(f"'{kind}: 이름'이 필요합니다", f"'{kind}: Name' is required"))
        return None
    schema = Schema(relative, str(top[kind].value).strip(), [], is_enum=is_enum, kind="yaml")
    if not _valid_name(schema, errors):
        return None
    for key, _ in root.value:
        if key.value not in (kind, list_key):
            errors.add(relative, str(key.start_mark.line + 1), tr(f"알 수 없는 키 '{key.value}'", f"unknown key '{key.value}'"))
    items = top.get(list_key)
    if not isinstance(items, yaml.SequenceNode):
        errors.add(relative, "1", tr(f"'{list_key}:' 목록이 필요합니다", f"a '{list_key}:' list is required"))
        return None
    keys = ("name", "value", "comment") if is_enum else ("name", "type", "scope", "comment")
    for item in items.value:
        line = item.start_mark.line + 1
        if not isinstance(item, yaml.MappingNode):
            errors.add(relative, str(line), tr(f"항목은 {'/'.join(keys)} 키를 가진 매핑이어야 합니다", f"an item must be a mapping with {'/'.join(keys)}"))
            continue
        values: dict[str, object] = {}
        for key, value in item.value:
            if key.value not in keys:
                errors.add(relative, str(key.start_mark.line + 1), tr(f"알 수 없는 키 '{key.value}'", f"unknown key '{key.value}'"))
            elif not isinstance(value, yaml.ScalarNode):
                errors.add(relative, str(value.start_mark.line + 1), tr(f"'{key.value}'에는 값 하나만 씁니다", f"'{key.value}' takes a single value"))
            else:
                schema.lines[(line, keys.index(key.value) + 1)] = value.start_mark.line + 1
                values[key.value] = None if value.tag.endswith(":null") else value.value
        if values.get("name") in (None, ""):
            errors.add(relative, str(line), tr("이름이 비어 있습니다", "the name is empty"))
            continue
        if is_enum:
            schema.rows.append((line, values["name"], values.get("value"), None, values.get("comment")))
            continue
        if values.get("scope") is None:
            # '#' starts a YAML comment, so an unquoted  scope: #  reads as empty.
            errors.add(relative, schema.cell(line, 3), tr(
                "scope가 비어 있습니다. '#'은 YAML 주석 기호라 scope: \"#\"처럼 따옴표로 감싸야 합니다",
                "scope is empty. '#' starts a YAML comment, so write it quoted: scope: \"#\"",
            ))
            continue
        schema.rows.append((line, values["name"], values.get("type"), values["scope"], values.get("comment")))
    return schema


def _valid_name(schema: Schema, errors: ErrorCollector) -> bool:
    if IDENTIFIER_RE.fullmatch(schema.name):
        return True
    errors.add(schema.where, "1" if schema.kind == "yaml" else "A1", tr(
        f"올바르지 않은 이름 '{schema.name}'", f"invalid name '{schema.name}'",
    ))
    return False
