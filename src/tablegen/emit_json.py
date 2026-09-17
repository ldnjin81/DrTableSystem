"""클라이언트·서버 JSON 생성."""

from __future__ import annotations

import json
from pathlib import Path

from .excel import DataModel
from .schema import ColumnSchema, EnumSchema, TableSchema


def emit_json(
    model: DataModel,
    client_output: Path,
    server_output: Path,
    stamp: str | None = None,
) -> None:
    client_output.mkdir(parents=True, exist_ok=True)
    server_output.mkdir(parents=True, exist_ok=True)
    enums = {enum.name: enum for enum in model.enums}
    for table in model.tables:
        rows = _sorted_rows(table, enums)
        _write_json(
            client_output / f"{table.name}.json",
            _table_payload(table, rows, {"B", "C"}, enums, with_index=True),
        )
        _write_json(
            server_output / f"{table.name}.json",
            _table_payload(table, rows, {"B", "S"}, enums, with_index=False),
        )
    manifest: dict[str, object] = {
        "source_files": list(model.source_files),
        "tables": [
            {"name": table.name, "rows": len(table.rows), "schema_hash": table.schema_hash}
            for table in model.tables
        ],
        "enums": [{"name": enum.name, "values": len(enum.values)} for enum in model.enums],
    }
    if stamp is not None:
        manifest["generated_at"] = stamp
    _write_json(client_output / "manifest.json", manifest)
    _write_json(server_output / "manifest.json", manifest)


def _sort_value(
    column: ColumnSchema,
    value: object,
    enums: dict[str, EnumSchema],
) -> object:
    """정렬 기준값. 열거형은 이름이 아니라 **값**으로 정렬해야 C++ 비교와 일치한다."""
    if column.type_name.startswith("E"):
        enum = enums.get(column.type_name[1:])
        if enum:
            for item in enum.values:
                if item.name == value:
                    return item.value
        return 0
    return value


def _sorted_rows(
    table: TableSchema,
    enums: dict[str, EnumSchema],
) -> list[dict[str, object]]:
    """행을 기본키 오름차순으로 정렬한다. 에셋의 PrimaryKeys도 이 순서를 따른다."""
    primary = table.primary_key
    return sorted(
        table.rows,
        key=lambda row: _sort_value(primary, row[primary.name], enums),
    )


def _sub_key_index(
    column: ColumnSchema,
    rows: list[dict[str, object]],
    enums: dict[str, EnumSchema],
) -> dict[str, object]:
    """CSR 인덱스. 버킷 순서는 키 값으로 정렬해 고정한다(딕셔너리 순회에 기대지 않는다)."""
    buckets: dict[object, list[int]] = {}
    for index, row in enumerate(rows):
        buckets.setdefault(row[column.name], []).append(index)
    keys = sorted(buckets, key=lambda key: _sort_value(column, key, enums))
    offsets = [0]
    indices: list[int] = []
    for key in keys:
        indices.extend(buckets[key])
        offsets.append(len(indices))
    return {"keys": list(keys), "offsets": offsets, "indices": indices}


def _table_payload(
    table: TableSchema,
    rows: list[dict[str, object]],
    scopes: set[str],
    enums: dict[str, EnumSchema],
    with_index: bool,
) -> dict[str, object]:
    included = [column for column in table.columns if column.scope in scopes]
    primary = table.primary_key
    payload: dict[str, object] = {
        "table": table.name,
        "schema_hash": table.schema_hash,
        "primary_key": primary.name,
    }
    if with_index:
        payload["primary_keys"] = [row[primary.name] for row in rows]
    sub_keys: list[dict[str, object]] = []
    for column in table.sub_keys:
        if column.scope not in scopes:
            continue
        entry: dict[str, object] = {"name": column.name, "field": column.name}
        if with_index:
            entry.update(_sub_key_index(column, rows, enums))
        sub_keys.append(entry)
    payload["sub_keys"] = sub_keys
    payload["rows"] = [
        {column.name: row[column.name] for column in included} for row in rows
    ]
    return payload


def _write_json(path: Path, payload: object) -> None:
    text = json.dumps(payload, ensure_ascii=False, indent=2) + "\n"
    path.write_text(text, encoding="utf-8", newline="\n")
