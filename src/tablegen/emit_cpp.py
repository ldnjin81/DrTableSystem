"""언리얼 C++ 헤더 생성."""

from __future__ import annotations

import json
from pathlib import Path

from .excel import DataModel
from .schema import ColumnSchema, EnumSchema, TableSchema
from .values import default_value

DEFAULT_ASSET_BASE = "UPrimaryDataAsset"
DEFAULT_ASSET_BASE_HEADER = "Engine/DataAsset.h"
CPP_TYPES = {
    "name": "FName",
    "string": "FString",
    "text": "FText",
    "tag": "FGameplayTag",
    "path": "FSoftObjectPath",
}
TYPE_INCLUDES = {
    "text": "Internationalization/Text.h",
    "tag": "GameplayTagContainer.h",
    "path": "UObject/SoftObjectPath.h",
}


def emit_cpp(
    model: DataModel,
    output: Path,
    prefix: str,
    asset_base: str = DEFAULT_ASSET_BASE,
    asset_base_header: str | None = None,
) -> None:
    output.mkdir(parents=True, exist_ok=True)
    header = asset_base_header or DEFAULT_ASSET_BASE_HEADER
    for enum in model.enums:
        _write(output / f"E{prefix}{enum.name}.h", _enum_header(enum, prefix))
    enums = {enum.name: enum for enum in model.enums}
    for table in model.tables:
        _write(
            output / f"{prefix}{table.name}Row.h",
            _table_header(table, prefix, enums),
        )
        _write(
            output / f"{prefix}{table.name}Table.h",
            _asset_header(table, prefix, asset_base, header),
        )
    _write(output / f"{prefix}GeneratedTables.h", _tables_header(model, prefix))


def _source_line(source_name: str, sheet: str) -> str:
    return f"// 자동 생성됨 — 직접 수정하지 말 것. 출처: {source_name} / {sheet}\n"


def _enum_header(enum: EnumSchema, prefix: str) -> str:
    lines = [
        _source_line(enum.source_name, enum.sheet).rstrip("\n"),
        "#pragma once",
        "",
        '#include "CoreMinimal.h"',
        f'#include "E{prefix}{enum.name}.generated.h"',
        "",
        "UENUM(BlueprintType)",
        f"enum class E{prefix}{enum.name} : uint8",
        "{",
    ]
    for value in enum.values:
        suffix = f" // {_cpp_comment(value.comment)}" if value.comment else ""
        lines.append(f"    {value.name} = {value.value},{suffix}")
    lines.extend(["};", ""])
    return "\n".join(lines)


def _table_header(
    table: TableSchema,
    prefix: str,
    enums: dict[str, EnumSchema],
) -> str:
    client_columns = [column for column in table.columns if column.scope in {"B", "C"}]
    enum_includes = sorted(
        {column.type_name[1:] for column in client_columns if column.type_name.startswith("E")}
    )
    type_includes = sorted(
        {TYPE_INCLUDES[column.type_name] for column in client_columns if column.type_name in TYPE_INCLUDES}
    )
    lines = [
        _source_line(table.source_name, table.sheet).rstrip("\n"),
        "#pragma once",
        "",
        '#include "CoreMinimal.h"',
        '#include "Engine/DataTable.h"',
    ]
    lines.extend(f'#include "{header}"' for header in type_includes)
    lines.extend(f'#include "E{prefix}{name}.h"' for name in enum_includes)
    lines.extend(
        [
            f'#include "{prefix}{table.name}Row.generated.h"',
            "",
            "USTRUCT(BlueprintType)",
            f"struct F{prefix}{table.name}Row : public FTableRowBase",
            "{",
            "    GENERATED_BODY()",
            "",
        ]
    )
    for column in table.columns:
        if column.scope not in {"B", "C"}:
            continue
        property_specifiers = (
            "EditAnywhere"
            if column.is_array
            else "EditAnywhere, BlueprintReadOnly"
        )
        metadata = ""
        if column.ref_target:
            metadata = f', meta = (TableRef = "{column.ref_target}"'
            if column.ref_key:
                metadata += f', TableRefKey = "{column.ref_key}"'
            metadata += ")"
        lines.append(
            f'    UPROPERTY({property_specifiers}, Category = "{prefix}|{table.name}"{metadata})'
        )
        declaration = f"{_cpp_type(column, prefix)} {column.name}"
        if column.is_array:
            declaration += f"[{column.array_size}]"
            if any(value is not None for value in column.default_values):
                values = [
                    value if value is not None else default_value(column.type_name, enums)
                    for value in column.default_values
                ]
                initializer = ", ".join(
                    _cpp_value(column.type_name, value, prefix) for value in values
                )
                declaration += f" = {{{initializer}}}"
            else:
                declaration += " = {}"
        else:
            declaration += _cpp_initializer(column, prefix, enums)
        lines.extend([f"    {declaration};", ""])
    lines.extend(["};", ""])
    return "\n".join(lines)


def _asset_header(
    table: TableSchema,
    prefix: str,
    asset_base: str,
    asset_base_header: str,
) -> str:
    """커밋릿이 구울 DataAsset 클래스. 인덱스는 빌드 타임에 계산돼 그대로 담긴다."""
    client_columns = [column for column in table.columns if column.scope in {"B", "C"}]
    enum_includes = sorted(
        {column.type_name[1:] for column in client_columns if column.type_name.startswith("E")}
    )
    lines = [
        _source_line(table.source_name, table.sheet).rstrip("\n"),
        "#pragma once",
        "",
        '#include "CoreMinimal.h"',
        f'#include "{asset_base_header}"',
        f'#include "{prefix}{table.name}Row.h"',
    ]
    lines.extend(f'#include "E{prefix}{name}.h"' for name in enum_includes)
    lines.extend(
        [
            f'#include "{prefix}{table.name}Table.generated.h"',
            "",
            "UCLASS(BlueprintType)",
            f"class U{prefix}{table.name}Table : public {asset_base}",
            "{",
            "    GENERATED_BODY()",
            "",
            "public:",
            "    // 행은 연속 배열 하나. 기본키 오름차순으로 정렬돼 있다.",
            f'    UPROPERTY(EditAnywhere, BlueprintReadOnly, Category = "{prefix}|{table.name}")',
            f"    TArray<F{prefix}{table.name}Row> Rows;",
            "",
            "    // 기본키 — Rows와 같은 순서라 이진 탐색이 된다.",
            "    UPROPERTY()",
            f"    TArray<{_cpp_type(table.primary_key, prefix)}> PrimaryKeys;",
            "",
        ]
    )
    for column in table.sub_keys:
        if column.scope not in {"B", "C"}:
            continue
        key_type = _cpp_type(column, prefix)
        lines.extend(
            [
                f"    // 서브키 {column.name} — CSR 인덱스(버킷 경계 + 행 인덱스).",
                "    UPROPERTY()",
                f"    TArray<{key_type}> {column.name}_Keys;",
                "",
                "    UPROPERTY()",
                f"    TArray<int32> {column.name}_Offsets;",
                "",
                "    UPROPERTY()",
                f"    TArray<int32> {column.name}_Indices;",
                "",
            ]
        )
    lines.extend(["};", ""])
    return "\n".join(lines)


def _tables_header(model: DataModel, prefix: str) -> str:
    source_names = ", ".join(model.source_files)
    lines = [
        f"// 자동 생성됨 — 직접 수정하지 말 것. 출처: {source_names} / 전체",
        "#pragma once",
        "",
        '#include "CoreMinimal.h"',
        "",
        f"namespace {prefix}GeneratedTables",
        "{",
    ]
    for table in model.tables:
        primary = table.primary_key
        sub_keys = ",".join(column.name for column in table.sub_keys)
        lines.extend(
            [
                f'    inline constexpr TCHAR {table.name}Name[] = TEXT("{table.name}");',
                f'    inline constexpr TCHAR {table.name}PrimaryKey[] = TEXT("{primary.name}");',
                f'    inline constexpr TCHAR {table.name}SubKeys[] = TEXT("{sub_keys}");',
                f'    inline constexpr TCHAR {table.name}SchemaHash[] = TEXT("{table.schema_hash}");',
                "",
            ]
        )
    lines.extend(["}", ""])
    return "\n".join(lines)


def _cpp_type(column: ColumnSchema, prefix: str) -> str:
    if column.type_name.startswith("E"):
        return f"E{prefix}{column.type_name[1:]}"
    return CPP_TYPES.get(column.type_name, column.type_name)


def _cpp_initializer(
    column: ColumnSchema,
    prefix: str,
    enums: dict[str, EnumSchema],
) -> str:
    if column.default_values and column.default_values[0] is not None:
        return f" = {_cpp_value(column.type_name, column.default_values[0], prefix)}"
    initializers = {
        "int32": " = 0",
        "int64": " = 0",
        "float": " = 0.0f",
        "double": " = 0.0",
        "bool": " = false",
    }
    if column.type_name in initializers:
        return initializers[column.type_name]
    if column.ref_target and column.type_name == "name":
        return " = NAME_None"
    if column.type_name.startswith("E"):
        enum_name = column.type_name[1:]
        first_value = enums[enum_name].values[0].name
        return f" = E{prefix}{enum_name}::{first_value}"
    return ""


def _cpp_value(type_name: str, value: object, prefix: str) -> str:
    if type_name in {"int32", "int64"}:
        return str(value)
    if type_name == "float":
        text = repr(float(value))
        return f"{text if '.' in text or 'e' in text else text + '.0'}f"
    if type_name == "double":
        text = repr(float(value))
        return text if "." in text or "e" in text else text + ".0"
    if type_name == "bool":
        return "true" if value else "false"
    literal = json.dumps(str(value), ensure_ascii=False)
    if type_name == "name":
        return f"FName(TEXT({literal}))"
    if type_name == "string":
        return f"FString(TEXT({literal}))"
    if type_name == "text":
        return f"FText::FromString(TEXT({literal}))"
    if type_name == "tag":
        # 멤버 기본 초기화자는 태그 테이블이 올라오기 전(CDO 생성 등)에도 실행될 수 있다.
        # ErrorIfNotFound 기본값 true는 그때 ensureAlwaysMsgf를 발동시키므로 false를 명시한다.
        return f"FGameplayTag::RequestGameplayTag(FName(TEXT({literal})), false)"
    if type_name == "path":
        return f"FSoftObjectPath(TEXT({literal}))"
    if type_name.startswith("E"):
        return f"E{prefix}{type_name[1:]}::{value}"
    raise ValueError(f"지원하지 않는 C++ 기본값 자료형: {type_name}")


def _cpp_comment(value: str) -> str:
    return value.replace("\r", " ").replace("\n", " ")


def _write(path: Path, content: str) -> None:
    path.write_text(content, encoding="utf-8", newline="\n")
