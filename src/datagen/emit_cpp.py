"""언리얼 C++ 헤더 생성."""

from __future__ import annotations

from pathlib import Path

from .excel import DataModel
from .schema import ColumnSchema, EnumSchema, TableSchema


def emit_cpp(model: DataModel, output: Path, prefix: str) -> None:
    output.mkdir(parents=True, exist_ok=True)
    for enum in model.enums:
        _write(output / f"E{prefix}{enum.name}.h", _enum_header(enum, prefix))
    enums = {enum.name: enum for enum in model.enums}
    for table in model.tables:
        _write(
            output / f"{prefix}{table.name}Row.h",
            _table_header(table, prefix, enums),
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
    enum_includes = sorted(
        {column.type_name[1:] for column in table.columns if column.type_name.startswith("E")}
    )
    lines = [
        _source_line(table.source_name, table.sheet).rstrip("\n"),
        "#pragma once",
        "",
        '#include "CoreMinimal.h"',
        '#include "Engine/DataTable.h"',
    ]
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
        lines.append(
            f'    UPROPERTY(EditAnywhere, BlueprintReadOnly, Category = "{prefix}|{table.name}")'
        )
        declaration = f"{_cpp_type(column, prefix)} {column.name}"
        if column.is_array:
            declaration += f"[{column.array_size}] = {{}}"
        else:
            declaration += _cpp_initializer(column, prefix, enums)
        lines.extend([f"    {declaration};", ""])
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
    return column.type_name


def _cpp_initializer(
    column: ColumnSchema,
    prefix: str,
    enums: dict[str, EnumSchema],
) -> str:
    initializers = {
        "int32": " = 0",
        "int64": " = 0",
        "float": " = 0.0f",
        "double": " = 0.0",
        "bool": " = false",
    }
    if column.type_name in initializers:
        return initializers[column.type_name]
    if column.type_name.startswith("E"):
        enum_name = column.type_name[1:]
        first_value = enums[enum_name].values[0].name
        return f" = E{prefix}{enum_name}::{first_value}"
    return ""


def _cpp_comment(value: str) -> str:
    return value.replace("\r", " ").replace("\n", " ")


def _write(path: Path, content: str) -> None:
    path.write_text(content, encoding="utf-8", newline="\n")
