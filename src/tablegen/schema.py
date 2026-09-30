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
REF_RE = re.compile(r"^ref\s*<\s*(?P<target>[^<>\s]+)\s*>$", re.IGNORECASE)
REF_ROLE_RE = re.compile(r"^(?P<role>id|subkey)\s*<\s*(?P<type>ref\s*<[^<>]+>)\s*>$", re.IGNORECASE)
ENUM_RE = re.compile(r"^E(?P<name>[A-Za-z][A-Za-z0-9_]*)$")
PRIMITIVES = {
    "int32",
    "int64",
    "float",
    "double",
    "bool",
    "name",
    "string",
    "text",
    "tag",
    "path",
}
LEGACY_TYPES = {"FName", "FString"}
KEY_PRIMITIVES = {"int32", "int64", "name"}
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
    default_values: tuple[object | None, ...] = ()
    ref_target: str | None = None
    ref_key: str | None = None

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
    default_text: str | None
    ref_target: str | None = None
    ref_key: str | None = None
    ref_scope: str | None = None


def parse_type(
    value: object, sheet: str, cell: str, errors: ErrorCollector,
    table_keys: dict[str, tuple[str, str, str]] | None = None,
    resolved_types: dict[str, str] | None = None,
) -> ParsedType | None:
    text = "" if value is None else str(value).strip()
    default_text: str | None = None
    if "=" in text:
        text, default_text = text.split("=", 1)
        text = text.strip()
        default_text = default_text.strip()
    role: str | None = None
    match = REF_ROLE_RE.fullmatch(text) or ROLE_RE.fullmatch(text)
    if match:
        role = match.group("role").lower()
        text = match.group("type").strip()
    ref_match = REF_RE.fullmatch(text)
    ref_spec = ref_match.group("target") if ref_match else None
    ref_target, _, ref_key = ref_spec.partition(".") if ref_spec else (None, "", "")
    ref_key = ref_key or None
    ref_scope = None
    if ref_target:
        if role == "id":
            errors.add(sheet, cell, "ID<Ref<T>>는 기본키로 사용할 수 없습니다")
            return None
        if default_text is not None:
            errors.add(sheet, cell, "Ref<T>에는 기본값을 지정할 수 없습니다")
            return None
        if table_keys is not None:
            if ref_target not in table_keys:
                errors.add(sheet, cell, f"참조 대상 테이블 '{ref_target}'이 없습니다")
                return None
            target_name = ref_spec
            if ref_key:
                if target_name not in table_keys:
                    errors.add(sheet, cell, f"참조 대상 필드 '{target_name}'이 없습니다")
                    return None
                _, target_role, ref_scope = table_keys[target_name]
                if target_role == "id":
                    errors.add(sheet, cell, f"기본키 '{target_name}'에는 Ref<{ref_target}>를 쓰세요")
                    return None
                if target_role != "subkey":
                    errors.add(sheet, cell, f"'{target_name}'을 SubKey로 선언하세요")
                    return None
            else:
                _, _, ref_scope = table_keys[ref_target]
            cache = resolved_types if resolved_types is not None else {}

            def resolve(spec: str, path: list[str]) -> str | None:
                if spec in cache:
                    return cache[spec]
                if spec in path:
                    cycle = path[path.index(spec):] + [spec]
                    errors.add(sheet, cell, f"자료형 순환: {' → '.join(cycle)}")
                    return None
                if spec not in table_keys:
                    errors.add(sheet, cell, f"참조 '{spec}'의 자료형을 결정할 수 없습니다")
                    return None
                candidate = table_keys[spec][0]
                if candidate.startswith("Ref<") and candidate.endswith(">"):
                    candidate = resolve(candidate[4:-1], [*path, spec])
                    if candidate is None:
                        return None
                cache[spec] = candidate
                return candidate

            text = resolve(ref_spec, [])
            if text is None:
                return None
        else:
            text = f"Ref<{ref_spec}>"
    if text in LEGACY_TYPES:
        errors.add(sheet, cell, f"옛 자료형 '{text}'은 지원하지 않습니다. 이제 name/string을 쓰세요")
        return None
    if text not in PRIMITIVES and not ENUM_RE.fullmatch(text) and not ref_target:
        errors.add(sheet, cell, f"알 수 없는 자료형 '{text}'")
        return None
    if role is not None and text not in KEY_PRIMITIVES and not ENUM_RE.fullmatch(text) and not ref_target:
        errors.add(
            sheet,
            cell,
            f"{text} 자료형은 기본키나 서브키로 사용할 수 없습니다. "
            "키에는 int32, int64, name, 열거형(E*)만 사용할 수 있습니다",
        )
        return None
    if role is not None and default_text is not None:
        errors.add(sheet, cell, "기본키와 서브키에는 기본값을 지정할 수 없습니다")
        default_text = None
    return ParsedType(text, role, default_text, ref_target, ref_key, ref_scope)


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
    table_keys: dict[str, tuple[str, str, str]] | None = None,
) -> list[ColumnSchema]:
    """물리 열을 논리 필드로 바꾸고 배열 열을 묶는다."""
    scalars: list[tuple[int, ColumnSchema]] = []
    arrays: dict[str, list[tuple[int, int, ParsedType | None, str, str]]] = {}
    scalar_headers: dict[str, str] = {}
    resolved_types: dict[str, str] = {}

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
        parsed = parse_type(raw_type, sheet, type_cell, errors, table_keys, resolved_types)
        if parsed:
            validate_enum_type(parsed.type_name, enums, sheet, type_cell, errors)
            if parsed.ref_key and parsed.ref_scope:
                output_scopes = {"B": {"C", "S"}, "C": {"C"}, "S": {"S"}}
                if not output_scopes[scope].issubset(output_scopes[parsed.ref_scope]):
                    errors.add(
                        sheet, type_cell,
                        f"참조 범위 {scope}가 대상 서브키 {parsed.ref_scope}보다 넓습니다",
                    )

        if array_match:
            index = int(array_match.group("index"))
            arrays.setdefault(base_name, []).append(
                (column_index, index, parsed, scope, header_cell)
            )
        elif parsed:
            default_value = _convert_default(parsed, enums, sheet, type_cell, errors)
            scalars.append(
                (
                    column_index,
                    ColumnSchema(
                        name,
                        parsed.type_name,
                        parsed.role,
                        scope,
                        (column_index,),
                        (header_cell,),
                        default_values=(default_value,),
                        ref_target=parsed.ref_target,
                        ref_key=parsed.ref_key,
                    ),
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
            if parsed and first_type and (parsed.type_name, parsed.ref_target, parsed.ref_key) != (first_type.type_name, first_type.ref_target, first_type.ref_key):
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
                        tuple(
                            _convert_default(
                                item[2], enums, sheet, _cell(item[0], 2), errors
                            )
                            if item[2] is not None
                            else None
                            for item in parts_by_index
                        ),
                        first_type.ref_target,
                        first_type.ref_key,
                    ),
                )
            )

    columns = [column for _, column in sorted(scalars + grouped, key=lambda item: item[0])]
    ids = [column for column in columns if column.role == "id"]
    if len(ids) != 1:
        errors.add(sheet, "A2", f"기본키는 정확히 1개여야 합니다(현재 {len(ids)}개)")
    elif ids[0].scope != "B":
        errors.add(
            sheet,
            _cell(ids[0].source_columns[0], 3),
            "기본키 범위는 B여야 합니다",
        )
    return columns


def _convert_default(
    parsed: ParsedType,
    enums: dict[str, EnumSchema],
    sheet: str,
    cell: str,
    errors: ErrorCollector,
) -> object | None:
    if parsed.default_text is None:
        return None
    # 순환 import를 피하면서 자료 셀과 완전히 같은 변환 규칙을 사용한다.
    from .values import convert_value

    return convert_value(
        parsed.default_text,
        parsed.type_name,
        enums,
        sheet,
        cell,
        errors,
        use_default_for_empty=False,
    )


def calculate_schema_hash(columns: list[ColumnSchema]) -> str:
    payload = []
    for column in columns:
        entry = {
            "name": column.name,
            "type": f"Ref<{column.ref_target}.{column.ref_key}>" if column.ref_key else (
                f"Ref<{column.ref_target}>" if column.ref_target else column.type_name
            ),
            "role": column.role,
            "scope": column.scope,
            "array_size": column.array_size,
        }
        if any(value is not None for value in column.default_values):
            entry["defaults"] = [
                {"declared": value is not None, "value": value}
                for value in column.default_values
            ]
        payload.append(entry)
    encoded = json.dumps(payload, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()
    return "sha256:" + hashlib.sha256(encoded).hexdigest()


def _cell(column: int, row: int) -> str:
    letters = ""
    while column:
        column, remainder = divmod(column - 1, 26)
        letters = chr(65 + remainder) + letters
    return f"{letters}{row}"
