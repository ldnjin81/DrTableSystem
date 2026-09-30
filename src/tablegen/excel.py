"""Reads .xlsx files into a validated data model."""

from __future__ import annotations

import re
from dataclasses import dataclass
from pathlib import Path
from xml.etree.ElementTree import ParseError
from zipfile import BadZipFile

from openpyxl import load_workbook
from openpyxl.utils import get_column_letter
from openpyxl.utils.exceptions import InvalidFileException

from .errors import ErrorCollector, ValidationErrors
from .i18n import tr
from .schema import (
    DATA_ROW,
    IDENTIFIER_RE,
    NAME_ROW,
    SCOPE_ROW,
    TYPE_ROW,
    ColumnSchema,
    EnumSchema,
    EnumValue,
    TableSchema,
    build_columns,
    calculate_schema_hash,
    parse_type,
)
from .values import convert_value

ENUM_SHEET_RE = re.compile(r"^<enum>(?P<name>[A-Za-z][A-Za-z0-9_]*)$")


@dataclass(frozen=True)
class DataModel:
    source_files: tuple[str, ...]
    enums: tuple[EnumSchema, ...]
    tables: tuple[TableSchema, ...]


def load_model(input_path: Path) -> DataModel:
    files = _find_files(input_path)
    errors = ErrorCollector()
    workbooks: list[tuple[Path, object]] = []
    for path in files:
        try:
            workbooks.append((path, load_workbook(path, data_only=True, read_only=True)))
        except (BadZipFile, InvalidFileException, KeyError, OSError, ParseError, ValueError) as exc:
            errors.add(path.name, "A1", tr(f"xlsx 파일을 읽을 수 없습니다: {exc}", f"cannot read the xlsx file: {exc}"))
    errors.raise_if_any()

    enum_sheets: dict[str, tuple[str, object]] = {}
    for path, workbook in workbooks:
        for sheet in workbook.worksheets:
            match = ENUM_SHEET_RE.fullmatch(sheet.title)
            if not match:
                continue
            name = match.group("name")
            if name in enum_sheets:
                errors.add(sheet.title, "A1", tr(f"열거형 '{name}'이 중복되었습니다", f"enum '{name}' is defined twice"))
            else:
                enum_sheets[name] = (path.name, sheet)

    # Register every enum name first so that enum sheets can reference each other.
    enum_names = {
        name: EnumSchema(name, sheet.title, source_name, ())
        for name, (source_name, sheet) in enum_sheets.items()
    }
    enums: dict[str, EnumSchema] = {}
    enum_columns: dict[str, list[ColumnSchema]] = {}
    for name, (source_name, sheet) in enum_sheets.items():
        enum, columns = _parse_enum(source_name, sheet, name, enum_names, errors)
        if enum:
            enums[name] = enum
            enum_columns[name] = columns

    # Resolve every table's primary key type before reading rows (Ref<T> needs them).
    table_keys: dict[str, tuple[str, str, str]] = {}
    for _, workbook in workbooks:
        for sheet in workbook.worksheets:
            if sheet.title.startswith("#") or ENUM_SHEET_RE.fullmatch(sheet.title):
                continue
            if not IDENTIFIER_RE.fullmatch(sheet.title):
                continue
            for index, raw_name, raw_type, raw_scope in _raw_columns(sheet):
                scope = str(raw_scope).strip().lower()
                if scope == "#":
                    continue
                if not re.match(r"^(id|subkey)\s*<", str(raw_type).strip(), re.IGNORECASE):
                    if IDENTIFIER_RE.fullmatch(str(raw_name)):
                        table_keys[f"{sheet.title}.{raw_name}"] = (str(raw_type), "field", scope)
                    continue
                parsed = parse_type(raw_type, sheet.title, f"{get_column_letter(index)}2", errors)
                if parsed and parsed.role == "id":
                    table_keys[sheet.title] = (parsed.type_name, "id", scope)
                    table_keys[f"{sheet.title}.{raw_name}"] = (parsed.type_name, "id", scope)
                elif parsed and parsed.role == "subkey":
                    table_keys[f"{sheet.title}.{raw_name}"] = (parsed.type_name, "subkey", scope)
    for name, columns in enum_columns.items():
        if any(column.role != "id" and column.name != "Value" for column in columns):
            info_name = f"{name}Info"
            table_keys[info_name] = (f"E{name}", "id", "all")
            for column in columns:
                if column.role == "id":
                    table_keys[f"{info_name}.{column.name}"] = (f"E{name}", "id", "all")
                elif column.name != "Value":
                    table_keys[f"{info_name}.{column.name}"] = (
                        column.type_name, column.role or "field", column.scope
                    )

    tables: list[TableSchema] = []
    table_names: set[str] = set()
    for path, workbook in workbooks:
        for sheet in workbook.worksheets:
            if sheet.title.startswith("#") or ENUM_SHEET_RE.fullmatch(sheet.title):
                continue
            if not IDENTIFIER_RE.fullmatch(sheet.title):
                errors.add(sheet.title, "A1", tr(f"올바르지 않은 테이블 이름 '{sheet.title}'", f"invalid table name '{sheet.title}'"))
                continue
            if sheet.title in table_names:
                errors.add(sheet.title, "A1", tr(f"테이블 '{sheet.title}'이 중복되었습니다", f"table '{sheet.title}' is defined twice"))
                continue
            table_names.add(sheet.title)
            table = _parse_table(path.name, sheet, enums, table_keys, errors)
            if table:
                tables.append(table)

    for name, (source_name, sheet) in enum_sheets.items():
        enum = enums.get(name)
        columns = enum_columns.get(name)
        if enum is None or columns is None:
            continue
        columns = build_columns(_raw_columns(sheet), sheet.title, enums, errors, table_keys)
        info_columns = [column for column in columns if column.name != "Value"]
        extras = [column for column in info_columns if column.role != "id"]
        if not extras:
            continue
        info_name = f"{name}Info"
        if info_name in table_names:
            errors.add(sheet.title, "A1", tr(f"Info 테이블 이름 '{info_name}'이 중복되었습니다", f"Info table name '{info_name}' is already used"))
            continue
        table_names.add(info_name)
        transformed = [
            ColumnSchema(
                column.name,
                f"E{name}" if column.role == "id" else column.type_name,
                column.role,
                column.scope,
                column.source_columns,
                column.header_cells,
                column.array_size,
                column.default_values,
                column.ref_target,
                column.ref_key,
            )
            for column in info_columns
        ]
        info_table = _read_table_rows(
            source_name, sheet, info_name, transformed, enums, errors
        )
        tables.append(info_table)

    for _, workbook in workbooks:
        workbook.close()
    errors.raise_if_any()
    return DataModel(
        tuple(path.name for path, _ in workbooks),
        tuple(sorted(enums.values(), key=lambda item: item.name)),
        tuple(sorted(tables, key=lambda item: item.name)),
    )


def _find_files(input_path: Path) -> list[Path]:
    if input_path.is_file() and input_path.suffix.lower() == ".xlsx":
        return [input_path]
    if input_path.is_dir():
        files = sorted(
            # Skip Excel lock files (~$Book.xlsx) that exist while a workbook is open.
            (
                path for path in input_path.iterdir()
                if path.suffix.lower() == ".xlsx" and not path.name.startswith("~$")
            ),
            key=lambda path: path.name,
        )
        if files:
            return files
    raise ValidationErrors([tr(f"입력!A1: xlsx 파일을 찾을 수 없습니다: {input_path}", f"input!A1: no xlsx file found: {input_path}")])


def _parse_enum(
    source_name: str,
    sheet: object,
    name: str,
    enum_names: dict[str, EnumSchema],
    errors: ErrorCollector,
) -> tuple[EnumSchema | None, list[ColumnSchema]]:
    raw_columns = _raw_columns(sheet)
    columns = build_columns(raw_columns, sheet.title, enum_names, errors)
    ids = [column for column in columns if column.role == "id"]
    if len(ids) != 1:
        return None, columns
    primary = ids[0]
    if primary.type_name != "name" or primary.is_array:
        errors.add(sheet.title, _cell_for(primary, TYPE_ROW), tr("열거형 기본키는 ID<name>이어야 합니다", "an enum sheet key must be ID<name>"))
    value_columns = [column for column in columns if column.name == "Value"]
    value_column = value_columns[0] if value_columns else None
    if value_column and (
        value_column.type_name != "int32" or value_column.role is not None or value_column.is_array
    ):
        errors.add(sheet.title, _cell_for(value_column, TYPE_ROW), tr("Value 열은 int32 일반 필드여야 합니다", "the Value column must be a plain int32 field"))
    comment_column = next(
        (
            column_index
            for column_index, raw_name, _, raw_scope in raw_columns
            if str(raw_name).strip() == "Comment"
            and str(raw_scope).strip().upper() == "#"
        ),
        None,
    )
    values: list[EnumValue] = []
    used_names: set[str] = set()
    used_values: set[int] = set()
    next_value = 0
    active_columns = {source for column in columns for source in column.source_columns}
    if comment_column:
        active_columns.add(comment_column)
    for row in range(DATA_ROW, sheet.max_row + 1):
        if all(sheet.cell(row, column).value in (None, "") for column in active_columns):
            continue
        raw_name = sheet.cell(row, primary.source_columns[0]).value
        raw_value = (
            sheet.cell(row, value_column.source_columns[0]).value if value_column else None
        )
        raw_comment = sheet.cell(row, comment_column).value if comment_column else None
        item_name = "" if raw_name is None else str(raw_name).strip()
        if not IDENTIFIER_RE.fullmatch(item_name):
            errors.add(
                sheet.title,
                f"{get_column_letter(primary.source_columns[0])}{row}",
                tr(f"올바르지 않은 열거자 이름 '{item_name}'", f"invalid enumerator name '{item_name}'"),
            )
            continue
        if item_name in used_names:
            errors.add(
                sheet.title,
                f"{get_column_letter(primary.source_columns[0])}{row}",
                tr(f"열거자 이름 '{item_name}'이 중복되었습니다", f"enumerator '{item_name}' appears twice"),
            )
        used_names.add(item_name)
        if raw_value in (None, ""):
            if value_column and value_column.default_values[0] is not None:
                value = int(value_column.default_values[0])
            else:
                value = next_value
        else:
            try:
                value = int(raw_value)
                if isinstance(raw_value, float) and not raw_value.is_integer():
                    raise ValueError
            except (TypeError, ValueError):
                cell = _cell_for(value_column, row) if value_column else "A1"
                errors.add(sheet.title, cell, tr(f"열거형 값 '{raw_value}'은 정수가 아닙니다", f"enum value '{raw_value}' is not an integer"))
                continue
        if not 0 <= value <= 255:
            cell = _cell_for(value_column, row) if value_column else _cell_for(primary, row)
            errors.add(sheet.title, cell, tr("열거형 값은 uint8 범위(0~255)여야 합니다", "enum values must fit in uint8 (0-255)"))
        if value in used_values:
            cell = _cell_for(value_column, row) if value_column else _cell_for(primary, row)
            errors.add(sheet.title, cell, tr(f"열거형 값 {value}가 중복되었습니다", f"enum value {value} appears twice"))
        used_values.add(value)
        next_value = value + 1
        values.append(EnumValue(item_name, value, "" if raw_comment is None else str(raw_comment)))
    if not values:
        errors.add(sheet.title, "A4", tr("열거형에는 항목이 하나 이상 필요합니다", "an enum needs at least one value"))
        return None, columns
    return EnumSchema(name, sheet.title, source_name, tuple(values)), columns


def _parse_table(
    source_name: str,
    sheet: object,
    enums: dict[str, EnumSchema],
    table_keys: dict[str, tuple[str, str, str]],
    errors: ErrorCollector,
) -> TableSchema | None:
    raw_columns = _raw_columns(sheet)
    columns = build_columns(raw_columns, sheet.title, enums, errors, table_keys)
    if not columns or not any(column.role == "id" for column in columns):
        return None
    return _read_table_rows(source_name, sheet, sheet.title, columns, enums, errors)


def _read_table_rows(
    source_name: str,
    sheet: object,
    table_name: str,
    columns: list[ColumnSchema],
    enums: dict[str, EnumSchema],
    errors: ErrorCollector,
) -> TableSchema:
    table = TableSchema(source_name=source_name, name=table_name, sheet=sheet.title, columns=columns)
    primary = table.primary_key
    used_keys: dict[object, str] = {}
    # (field, lowercase value) -> (first spelling, cell). Unreal FName is case-insensitive,
    # so 'Sword' and 'sword' would collapse into one key and break the sort contract.
    name_spellings: dict[tuple[str, str], tuple[str, str]] = {}
    data_source_columns = {
        source_column for column in columns for source_column in column.source_columns
    }
    for row in range(DATA_ROW, sheet.max_row + 1):
        if all(sheet.cell(row, index).value in (None, "") for index in data_source_columns):
            continue
        converted: dict[str, object] = {}
        for column in columns:
            if column.is_array:
                for source_column in column.source_columns:
                    raw_value = sheet.cell(row, source_column).value
                    if column.ref_target and column.type_name.startswith("E") and raw_value in (None, ""):
                        errors.add(
                            sheet.title,
                            f"{get_column_letter(source_column)}{row}",
                            tr("열거형 기본키를 참조하는 셀은 비울 수 없습니다", "a reference to an enum-keyed table cannot be empty"),
                        )
                converted[column.name] = [
                    convert_value(
                        _value_with_default(
                            sheet.cell(row, source_column).value,
                            column.default_values[position],
                        ),
                        column.type_name,
                        enums,
                        sheet.title,
                        f"{get_column_letter(source_column)}{row}",
                        errors,
                    )
                    for position, source_column in enumerate(column.source_columns)
                ]
            else:
                source_column = column.source_columns[0]
                raw_value = sheet.cell(row, source_column).value
                if column.ref_target and column.type_name.startswith("E") and raw_value in (None, ""):
                    errors.add(
                        sheet.title,
                        f"{get_column_letter(source_column)}{row}",
                        tr("열거형 기본키를 참조하는 셀은 비울 수 없습니다", "a reference to an enum-keyed table cannot be empty"),
                    )
                if column.role == "id" and raw_value in (None, ""):
                    errors.add(
                        sheet.title,
                        f"{get_column_letter(source_column)}{row}",
                        tr("기본키 값이 비어 있습니다", "the primary key is empty"),
                    )
                converted[column.name] = convert_value(
                    _value_with_default(raw_value, column.default_values[0]),
                    column.type_name,
                    enums,
                    sheet.title,
                    f"{get_column_letter(source_column)}{row}",
                    errors,
                )
        key = converted[primary.name]
        key_cell = f"{get_column_letter(primary.source_columns[0])}{row}"
        if key in used_keys:
            errors.add(sheet.title, key_cell, tr(f"기본키 값 '{key}'이 중복되었습니다", f"duplicate primary key '{key}'"))
        else:
            used_keys[key] = key_cell
            if primary.type_name == "name":
                _check_name_case(sheet.title, primary.name, key, key_cell, name_spellings, errors)
        for column in table.sub_keys:
            if column.type_name == "name" and column.name != primary.name:
                cell = f"{get_column_letter(column.source_columns[0])}{row}"
                _check_name_case(sheet.title, column.name, converted[column.name], cell, name_spellings, errors)
        table.rows.append(converted)
    table.schema_hash = calculate_schema_hash(columns, enums)
    return table


def _check_name_case(
    sheet: str,
    field: str,
    value: object,
    cell: str,
    spellings: dict[tuple[str, str], tuple[str, str]],
    errors: ErrorCollector,
) -> None:
    """Reports name key values that differ from an earlier value only by case."""
    if not isinstance(value, str) or value == "":
        return
    folded = (field, value.lower())
    seen = spellings.get(folded)
    if seen is None:
        spellings[folded] = (value, cell)
    elif seen[0] != value:
        errors.add(
            sheet,
            cell,
            tr(
                f"name 키 '{value}'이 {seen[1]}의 '{seen[0]}'과 대소문자만 다릅니다. "
                "언리얼 FName은 대소문자를 구분하지 않아 같은 값이 됩니다",
                f"name key '{value}' differs from '{seen[0]}' at {seen[1]} only by case. "
                "Unreal FName is case-insensitive, so they would be the same value",
            ),
        )


def _value_with_default(value: object, declared_default: object | None) -> object:
    if value in (None, "") and declared_default is not None:
        return declared_default
    return value


def _raw_columns(sheet: object) -> list[tuple[int, object, object, object]]:
    columns: list[tuple[int, object, object, object]] = []
    for column in range(1, sheet.max_column + 1):
        name = sheet.cell(NAME_ROW, column).value
        if name in (None, ""):
            break
        columns.append(
            (column, name, sheet.cell(TYPE_ROW, column).value, sheet.cell(SCOPE_ROW, column).value)
        )
    return columns


def _cell_for(column: ColumnSchema | None, row: int) -> str:
    if column is None:
        return "A1"
    return f"{get_column_letter(column.source_columns[0])}{row}"
