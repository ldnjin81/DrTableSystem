"""엑셀 스키마 자료형과 검증 규칙."""

from __future__ import annotations

import hashlib
import json
import re
from dataclasses import dataclass, field

from .errors import ErrorCollector

IDENTIFIER_RE = re.compile(r"^[A-Za-z][A-Za-z0-9_]*$")
ARRAY_RE = re.compile(r"^(?P<name>[A-Za-z][A-Za-z0-9_]*)\[(?P<index>\d+)]$")
ROLE_RE = re.compile(r"^(?P<role>id|subkey)\s*<\s*(?P<type>[^<>]+)\s*>$", re.IGNORECASE)
ENUM_RE = re.compile(r"^E(?P<name>[A-Za-z][A-Za-z0-9_]*)$")
PRIMITIVES = {"int32", "int64", "float", "double", "bool", "FName", "FString"}
SCOPES = {"B", "C", "S", "#"}


@dataclass(frozen=True)
class EnumValue:
    name: str
    value: int
    comment: str


@dataclass(frozen=True)
class EnumSchema:
    name: str
    sheet: str
    source_name: str
    values: tuple[EnumValue, ...]


@dataclass(frozen=True)
class ColumnSchema:
    name: str
    type_name: str
    role: str | None
    scope: str
    source_columns: tuple[int, ...]
    header_cells: tuple[str, ...]
    array_size: int | None = None

    @property
    def is_array(self) -> bool:
        return self.array_size is not None


@dataclass
class TableSchema:
    name: str
    sheet: str
    source_name: str
    columns: list[ColumnSchema]
    rows: list[dict[str, object]] = field(default_factory=list)
    schema_hash: str = ""

    @property
    def primary_key(self) -> ColumnSchema:
        return next(column for column in self.columns if column.role == "id")

    @property
    def sub_keys(self) -> list[ColumnSchema]:
        return [column for column in self.columns if column.role == "subkey"]


@dataclass(frozen=True)
class ParsedType:
    type_name: str
    role: str | None


def parse_type(value: object, sheet: str, cell: str, errors: ErrorCollector) -> ParsedType | None:
    text = "" if value is None else str(value).strip()
    role: str | None = None
    match = ROLE_RE.fullmatch(text)
    if match:
        role = match.group("role").lower()
        text = match.group("type").strip()
    if text not in PRIMITIVES and not ENUM_RE.fullmatch(text):
        errors.add(sheet, cell, f"알 수 없는 자료형 '{text}'")
        return None
    return ParsedType(text, role)


def validate_enum_type(
    type_name: str,
    enums: dict[str, EnumSchema],
    sheet: str,
    cell: str,
    errors: ErrorCollector,
) -> None:
    match = ENUM_RE.fullmatch(type_name)
    if match and match.group("name") not in enums:
        errors.add(sheet, cell, f"정의되지 않은 열거형 '{match.group('name')}'")


def build_columns(
    raw_columns: list[tuple[int, object, object, object]],
    sheet: str,
    enums: dict[str, EnumSchema],
    errors: ErrorCollector,
) -> list[ColumnSchema]:
    """물리 열을 논리 필드로 바꾸고 배열 열을 묶는다."""
    scalars: list[tuple[int, ColumnSchema]] = []
    arrays: dict[str, list[tuple[int, int, ParsedType | None, str, str]]] = {}
    scalar_headers: dict[str, str] = {}

    for column_index, raw_name, raw_type, raw_scope in raw_columns:
        header_cell = _cell(column_index, 1)
        type_cell = _cell(column_index, 2)
        scope_cell = _cell(column_index, 3)
        name = str(raw_name).strip()
        scope = "" if raw_scope is None else str(raw_scope).strip().upper()
        if scope not in SCOPES:
            errors.add(sheet, scope_cell, f"범위는 B, C, S, # 중 하나여야 합니다: '{scope}'")
            continue
        if scope == "#":
            continue

        array_match = ARRAY_RE.fullmatch(name)
        base_name = array_match.group("name") if array_match else name
        if not array_match and not IDENTIFIER_RE.fullmatch(name):
            errors.add(sheet, header_cell, f"올바르지 않은 필드명 '{name}'")
            continue
        parsed = parse_type(raw_type, sheet, type_cell, errors)
        if parsed:
            validate_enum_type(parsed.type_name, enums, sheet, type_cell, errors)

        if array_match:
            index = int(array_match.group("index"))
            arrays.setdefault(base_name, []).append(
                (column_index, index, parsed, scope, header_cell)
            )
        elif parsed:
            scalars.append(
                (
                    column_index,
                    ColumnSchema(name, parsed.type_name, parsed.role, scope, (column_index,), (header_cell,)),
                )
            )

        if not array_match:
            if base_name in scalar_headers:
                errors.add(sheet, header_cell, f"필드명 '{base_name}'이 중복되었습니다")
            else:
                scalar_headers[base_name] = header_cell

    grouped: list[tuple[int, ColumnSchema]] = []
    scalar_names = {column.name for _, column in scalars}
    for name, parts in arrays.items():
        parts_by_position = sorted(parts, key=lambda item: item[0])
        parts_by_index = sorted(parts, key=lambda item: (item[1], item[0]))
        first_column, _, first_type, first_scope, first_cell = parts_by_index[0]
        group_cell = parts_by_position[0][4]
        if name in scalar_names:
            errors.add(sheet, first_cell, f"필드명 '{name}'이 스칼라와 배열로 중복되었습니다")
        seen: set[int] = set()
        for _, index, _, _, cell in parts_by_position:
            if index in seen:
                errors.add(sheet, cell, f"배열 '{name}'의 인덱스 {index}가 중복되었습니다")
            seen.add(index)
        expected = list(range(len(seen)))
        if sorted(seen) != expected:
            errors.add(sheet, group_cell, f"배열 '{name}'의 인덱스는 0부터 연속이어야 합니다")
        if first_type and first_type.role:
            errors.add(sheet, _cell(first_column, 2), f"배열 '{name}'은 키로 지정할 수 없습니다")
        for column_index, _, parsed, scope, _ in parts_by_index[1:]:
            if parsed and first_type and parsed.type_name != first_type.type_name:
                errors.add(sheet, _cell(column_index, 2), f"배열 '{name}'의 자료형이 일치하지 않습니다")
            if parsed and parsed.role:
                errors.add(sheet, _cell(column_index, 2), f"배열 '{name}'은 키로 지정할 수 없습니다")
            if scope != first_scope:
                errors.add(sheet, _cell(column_index, 3), f"배열 '{name}'의 범위가 일치하지 않습니다")
        if first_type:
            grouped.append(
                (
                    min(item[0] for item in parts),
                    ColumnSchema(
                        name,
                        first_type.type_name,
                        None,
                        first_scope,
                        tuple(item[0] for item in parts_by_index),
                        tuple(item[4] for item in parts_by_index),
                        len(seen),
                    ),
                )
            )

    columns = [column for _, column in sorted(scalars + grouped, key=lambda item: item[0])]
    ids = [column for column in columns if column.role == "id"]
    if len(ids) != 1:
        errors.add(sheet, "A2", f"기본키는 정확히 1개여야 합니다(현재 {len(ids)}개)")
    return columns


def calculate_schema_hash(columns: list[ColumnSchema]) -> str:
    payload = [
        {
            "name": column.name,
            "type": column.type_name,
            "role": column.role,
            "scope": column.scope,
            "array_size": column.array_size,
        }
        for column in columns
    ]
    encoded = json.dumps(payload, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()
    return "sha256:" + hashlib.sha256(encoded).hexdigest()


def _cell(column: int, row: int) -> str:
    letters = ""
    while column:
        column, remainder = divmod(column - 1, 26)
        letters = chr(65 + remainder) + letters
    return f"{letters}{row}"
