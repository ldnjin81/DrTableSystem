"""Spreadsheet schema types and validation rules."""

from __future__ import annotations

import hashlib
import json
import re
from dataclasses import dataclass, field

from .errors import ErrorCollector
from .i18n import tr

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
SCOPES = {"all", "client", "server", "#"}
CLIENT_SCOPES = frozenset({"all", "client"})
SERVER_SCOPES = frozenset({"all", "server"})
# Header layout: row 1 field name, row 2 type, row 3 scope, data from row 4.
NAME_ROW, TYPE_ROW, SCOPE_ROW, DATA_ROW = 1, 2, 3, 4
# Old single-letter scope codes, rejected with a hint to the new words.
LEGACY_SCOPES = {"B": "all", "C": "client", "S": "server"}


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
class TableSource:
    """One sheet that contributes rows to a table (a table may be split over sheets and files)."""

    file: str
    sheet: str
    rows: int = 0

    @property
    def location(self) -> str:
        """Excel-style reference used in messages: [File.xlsx]Sheet."""
        return f"[{self.file}]{self.sheet}"


@dataclass
class TableSchema:
    name: str
    sheet: str
    source_name: str
    columns: list[ColumnSchema]
    rows: list[dict[str, object]] = field(default_factory=list)
    schema_hash: str = ""
    sources: list[TableSource] = field(default_factory=list)

    @property
    def location(self) -> str:
        """Where the table's header is defined (its first part)."""
        return self.sources[0].location if self.sources else f"[{self.source_name}]{self.sheet}"

    @property
    def source_list(self) -> list[str]:
        """Every part for generated comments: ['File.xlsx / Sheet', 'Other.xlsx / Sheet@Part']."""
        parts = self.sources or [TableSource(self.source_name, self.sheet)]
        return [f"{part.file} / {part.sheet}" for part in parts]

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
            errors.add(sheet, cell, tr("ID<Ref<T>>는 기본키로 사용할 수 없습니다", "ID<Ref<T>> cannot be a primary key"))
            return None
        if default_text is not None:
            errors.add(sheet, cell, tr("Ref<T>에는 기본값을 지정할 수 없습니다", "Ref<T> cannot have a default value"))
            return None
        if table_keys is not None:
            if ref_target not in table_keys:
                errors.add(sheet, cell, tr(f"참조 대상 테이블 '{ref_target}'이 없습니다", f"referenced table '{ref_target}' does not exist"))
                return None
            target_name = ref_spec
            if ref_key:
                if target_name not in table_keys:
                    errors.add(sheet, cell, tr(f"참조 대상 필드 '{target_name}'이 없습니다", f"referenced field '{target_name}' does not exist"))
                    return None
                _, target_role, ref_scope = table_keys[target_name]
                if target_role == "id":
                    errors.add(sheet, cell, tr(f"기본키 '{target_name}'에는 Ref<{ref_target}>를 쓰세요", f"'{target_name}' is the primary key; use Ref<{ref_target}>"))
                    return None
                if target_role != "subkey":
                    errors.add(sheet, cell, tr(f"'{target_name}'을 SubKey로 선언하세요", f"declare '{target_name}' as a SubKey to reference it"))
                    return None
            else:
                _, _, ref_scope = table_keys[ref_target]
            cache = resolved_types if resolved_types is not None else {}

            def resolve(spec: str, path: list[str]) -> str | None:
                if spec in cache:
                    return cache[spec]
                if spec in path:
                    cycle = path[path.index(spec):] + [spec]
                    errors.add(sheet, cell, tr(f"자료형 순환: {' → '.join(cycle)}", f"type cycle: {' → '.join(cycle)}"))
                    return None
                if spec not in table_keys:
                    errors.add(sheet, cell, tr(f"참조 '{spec}'의 자료형을 결정할 수 없습니다", f"cannot resolve the type of reference '{spec}'"))
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
        errors.add(sheet, cell, tr(f"옛 자료형 '{text}'은 지원하지 않습니다. 이제 name/string을 쓰세요", f"legacy type '{text}' is no longer supported; use name/string"))
        return None
    if text not in PRIMITIVES and not ENUM_RE.fullmatch(text) and not ref_target:
        errors.add(sheet, cell, tr(f"알 수 없는 자료형 '{text}'", f"unknown type '{text}'"))
        return None
    if role is not None and text not in KEY_PRIMITIVES and not ENUM_RE.fullmatch(text) and not ref_target:
        errors.add(
            sheet,
            cell,
            tr(
                f"{text} 자료형은 기본키나 서브키로 사용할 수 없습니다. "
                "키에는 int32, int64, name, 열거형(E*)만 사용할 수 있습니다",
                f"{text} cannot be a primary key or sub key. "
                "Keys must be int32, int64, name or an enum (E*)",
            ),
        )
        return None
    if role is not None and default_text is not None:
        errors.add(sheet, cell, tr("기본키와 서브키에는 기본값을 지정할 수 없습니다", "primary keys and sub keys cannot have a default value"))
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
        errors.add(sheet, cell, tr(f"정의되지 않은 열거형 '{match.group('name')}'", f"undefined enum '{match.group('name')}'"))


def build_columns(
    raw_columns: list[tuple[int, object, object, object]],
    sheet: str,
    enums: dict[str, EnumSchema],
    errors: ErrorCollector,
    table_keys: dict[str, tuple[str, str, str]] | None = None,
) -> list[ColumnSchema]:
    """Turns physical columns into logical fields, grouping Name[0], Name[1], ... into arrays."""
    scalars: list[tuple[int, ColumnSchema]] = []
    arrays: dict[str, list[tuple[int, int, ParsedType | None, str, str]]] = {}
    scalar_headers: dict[str, str] = {}
    resolved_types: dict[str, str] = {}

    for column_index, raw_name, raw_type, raw_scope in raw_columns:
        header_cell = _cell(column_index, NAME_ROW)
        type_cell = _cell(column_index, TYPE_ROW)
        scope_cell = _cell(column_index, SCOPE_ROW)
        name = str(raw_name).strip()
        raw_scope_text = "" if raw_scope is None else str(raw_scope).strip()
        scope = raw_scope_text.lower() if raw_scope_text.lower() in SCOPES else ""
        if not scope:
            if raw_scope_text.upper() in LEGACY_SCOPES:
                word = LEGACY_SCOPES[raw_scope_text.upper()]
                errors.add(sheet, scope_cell, tr(
                    f"옛 범위 표기 '{raw_scope_text}' 대신 '{word}'를 쓰세요",
                    f"use '{word}' instead of the old scope code '{raw_scope_text}'",
                ))
            else:
                errors.add(sheet, scope_cell, tr(
                    f"범위는 all, client, server, # 중 하나여야 합니다: '{raw_scope_text}'",
                    f"scope must be one of all, client, server, #: '{raw_scope_text}'",
                ))
            continue
        if scope == "#":
            continue

        array_match = ARRAY_RE.fullmatch(name)
        base_name = array_match.group("name") if array_match else name
        if not array_match and not IDENTIFIER_RE.fullmatch(name):
            errors.add(sheet, header_cell, tr(f"올바르지 않은 필드명 '{name}'", f"invalid field name '{name}'"))
            continue
        parsed = parse_type(raw_type, sheet, type_cell, errors, table_keys, resolved_types)
        if parsed:
            validate_enum_type(parsed.type_name, enums, sheet, type_cell, errors)
            if parsed.ref_key and parsed.ref_scope:
                output_scopes = {"all": {"client", "server"}, "client": {"client"}, "server": {"server"}}
                if not output_scopes[scope].issubset(output_scopes[parsed.ref_scope]):
                    errors.add(
                        sheet, type_cell,
                        tr(f"참조 범위 {scope}가 대상 서브키 범위 {parsed.ref_scope}보다 넓습니다", f"reference scope {scope} is wider than the target sub key scope {parsed.ref_scope}"),
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
                errors.add(sheet, header_cell, tr(f"필드명 '{base_name}'이 중복되었습니다", f"duplicate field name '{base_name}'"))
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
            errors.add(sheet, first_cell, tr(f"필드명 '{name}'이 스칼라와 배열로 중복되었습니다", f"field '{name}' is used both as a scalar and as an array"))
        seen: set[int] = set()
        for _, index, _, _, cell in parts_by_position:
            if index in seen:
                errors.add(sheet, cell, tr(f"배열 '{name}'의 인덱스 {index}가 중복되었습니다", f"array '{name}' has index {index} twice"))
            seen.add(index)
        expected = list(range(len(seen)))
        if sorted(seen) != expected:
            errors.add(sheet, group_cell, tr(f"배열 '{name}'의 인덱스는 0부터 연속이어야 합니다", f"array '{name}' indices must start at 0 without gaps"))
        if first_type and first_type.role:
            errors.add(sheet, _cell(first_column, TYPE_ROW), tr(f"배열 '{name}'은 키로 지정할 수 없습니다", f"array '{name}' cannot be a key"))
        for column_index, _, parsed, scope, _ in parts_by_index[1:]:
            if parsed and first_type and (parsed.type_name, parsed.ref_target, parsed.ref_key) != (first_type.type_name, first_type.ref_target, first_type.ref_key):
                errors.add(sheet, _cell(column_index, TYPE_ROW), tr(f"배열 '{name}'의 자료형이 일치하지 않습니다", f"array '{name}' elements have different types"))
            if parsed and parsed.role:
                errors.add(sheet, _cell(column_index, TYPE_ROW), tr(f"배열 '{name}'은 키로 지정할 수 없습니다", f"array '{name}' cannot be a key"))
            if scope != first_scope:
                errors.add(sheet, _cell(column_index, SCOPE_ROW), tr(f"배열 '{name}'의 범위가 일치하지 않습니다", f"array '{name}' elements have different scopes"))
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
                                item[2], enums, sheet, _cell(item[0], TYPE_ROW), errors
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
        errors.add(sheet, _cell(1, TYPE_ROW), tr(f"기본키는 정확히 1개여야 합니다(현재 {len(ids)}개)", f"exactly one primary key is required (found {len(ids)})"))
    elif ids[0].scope != "all":
        errors.add(
            sheet,
            _cell(ids[0].source_columns[0], SCOPE_ROW),
            tr("기본키 범위는 all이어야 합니다", "the primary key scope must be all"),
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
    # Imported here to avoid a cycle; defaults use exactly the same conversion as data cells.
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


def calculate_schema_hash(
    columns: list[ColumnSchema],
    enums: dict[str, EnumSchema] | None = None,
) -> str:
    """Structure hash: fields, key roles, scopes, defaults **and the (name, value) list of
    every enum the table uses**.

    Baked enum key indices are sorted by enum value, so changing enum values without
    re-baking would make the runtime binary search miss silently. Including the enum
    definitions turns that case into a schema mismatch.
    """
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
    used_enums = sorted(
        {column.type_name[1:] for column in columns if ENUM_RE.fullmatch(column.type_name)}
    )
    if enums is not None and used_enums:
        payload = {
            "columns": payload,
            "enums": {
                name: [[value.name, value.value] for value in enums[name].values]
                for name in used_enums
                if name in enums
            },
        }
    encoded = json.dumps(payload, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()
    return "sha256:" + hashlib.sha256(encoded).hexdigest()


def _cell(column: int, row: int) -> str:
    letters = ""
    while column:
        column, remainder = divmod(column - 1, 26)
        letters = chr(65 + remainder) + letters
    return f"{letters}{row}"
