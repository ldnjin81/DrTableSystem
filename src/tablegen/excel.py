"""xlsx 파일을 읽어 검증된 모델로 변환한다."""

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
from .schema import (
    IDENTIFIER_RE,
    ColumnSchema,
    EnumSchema,
    EnumValue,
    TableSchema,
    build_columns,
    calculate_schema_hash,
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
            errors.add(path.name, "A1", f"xlsx 파일을 읽을 수 없습니다: {exc}")
    errors.raise_if_any()

    enum_sheets: dict[str, tuple[str, object]] = {}
    for path, workbook in workbooks:
        for sheet in workbook.worksheets:
            match = ENUM_SHEET_RE.fullmatch(sheet.title)
            if not match:
                continue
            name = match.group("name")
            if name in enum_sheets:
                errors.add(sheet.title, "A1", f"열거형 '{name}'이 중복되었습니다")
            else:
                enum_sheets[name] = (path.name, sheet)

    # 모든 열거형 이름을 먼저 등록하여 열거형 시트의 부가 열도 서로 참조할 수 있게 한다.
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

    tables: list[TableSchema] = []
    table_names: set[str] = set()
    for path, workbook in workbooks:
        for sheet in workbook.worksheets:
            if sheet.title.startswith("#") or ENUM_SHEET_RE.fullmatch(sheet.title):
                continue
            if not IDENTIFIER_RE.fullmatch(sheet.title):
                errors.add(sheet.title, "A1", f"올바르지 않은 테이블 이름 '{sheet.title}'")
                continue
            if sheet.title in table_names:
                errors.add(sheet.title, "A1", f"테이블 '{sheet.title}'이 중복되었습니다")
                continue
            table_names.add(sheet.title)
            table = _parse_table(path.name, sheet, enums, errors)
            if table:
                tables.append(table)

    for name, (source_name, sheet) in enum_sheets.items():
        enum = enums.get(name)
        columns = enum_columns.get(name)
        if enum is None or columns is None:
            continue
        info_columns = [column for column in columns if column.name != "Value"]
        extras = [column for column in info_columns if column.role != "id"]
        if not extras:
            continue
        info_name = f"{name}Info"
        if info_name in table_names:
            errors.add(sheet.title, "A1", f"Info 테이블 이름 '{info_name}'이 중복되었습니다")
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
            (path for path in input_path.iterdir() if path.suffix.lower() == ".xlsx"),
            key=lambda path: path.name,
        )
        if files:
            return files
    raise ValidationErrors([f"입력!A1: xlsx 파일을 찾을 수 없습니다: {input_path}"])


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
        errors.add(sheet.title, _cell_for(primary, 2), "열거형 기본키는 ID<name>이어야 합니다")
    value_columns = [column for column in columns if column.name == "Value"]
    value_column = value_columns[0] if value_columns else None
    if value_column and (
        value_column.type_name != "int32" or value_column.role is not None or value_column.is_array
    ):
        errors.add(sheet.title, _cell_for(value_column, 2), "Value 열은 int32 일반 필드여야 합니다")
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
    for row in range(4, sheet.max_row + 1):
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
                f"올바르지 않은 열거자 이름 '{item_name}'",
            )
            continue
        if item_name in used_names:
            errors.add(
                sheet.title,
                f"{get_column_letter(primary.source_columns[0])}{row}",
                f"열거자 이름 '{item_name}'이 중복되었습니다",
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
                errors.add(sheet.title, cell, f"열거형 값 '{raw_value}'은 정수가 아닙니다")
                continue
        if not 0 <= value <= 255:
            cell = _cell_for(value_column, row) if value_column else _cell_for(primary, row)
            errors.add(sheet.title, cell, "열거형 값은 uint8 범위(0~255)여야 합니다")
        if value in used_values:
            cell = _cell_for(value_column, row) if value_column else _cell_for(primary, row)
            errors.add(sheet.title, cell, f"열거형 값 {value}가 중복되었습니다")
        used_values.add(value)
        next_value = value + 1
        values.append(EnumValue(item_name, value, "" if raw_comment is None else str(raw_comment)))
    if not values:
        errors.add(sheet.title, "A4", "열거형에는 항목이 하나 이상 필요합니다")
        return None, columns
    return EnumSchema(name, sheet.title, source_name, tuple(values)), columns


def _parse_table(
    source_name: str,
    sheet: object,
    enums: dict[str, EnumSchema],
    errors: ErrorCollector,
) -> TableSchema | None:
    raw_columns = _raw_columns(sheet)
    columns = build_columns(raw_columns, sheet.title, enums, errors)
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
    data_source_columns = {
        source_column for column in columns for source_column in column.source_columns
    }
    for row in range(4, sheet.max_row + 1):
        if all(sheet.cell(row, index).value in (None, "") for index in data_source_columns):
            continue
        converted: dict[str, object] = {}
        for column in columns:
            if column.is_array:
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
                if column.role == "id" and raw_value in (None, ""):
                    errors.add(
                        sheet.title,
                        f"{get_column_letter(source_column)}{row}",
                        "기본키 값이 비어 있습니다",
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
            errors.add(sheet.title, key_cell, f"기본키 값 '{key}'이 중복되었습니다")
        else:
            used_keys[key] = key_cell
        table.rows.append(converted)
    table.schema_hash = calculate_schema_hash(columns)
    return table


def _value_with_default(value: object, declared_default: object | None) -> object:
    if value in (None, "") and declared_default is not None:
        return declared_default
    return value


def _raw_columns(sheet: object) -> list[tuple[int, object, object, object]]:
    columns: list[tuple[int, object, object, object]] = []
    for column in range(1, sheet.max_column + 1):
        name = sheet.cell(1, column).value
        if name in (None, ""):
            break
        columns.append(
            (column, name, sheet.cell(2, column).value, sheet.cell(3, column).value)
        )
    return columns


def _cell_for(column: ColumnSchema | None, row: int) -> str:
    if column is None:
        return "A1"
    return f"{get_column_letter(column.source_columns[0])}{row}"
