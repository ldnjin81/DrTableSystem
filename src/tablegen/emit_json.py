"""클라이언트·서버 JSON 생성."""

from __future__ import annotations

import hashlib
import json
from pathlib import Path

from .excel import DataModel
from .schema import ColumnSchema, EnumSchema, TableSchema

CLIENT_SCOPES = {"B", "C"}
SERVER_SCOPES = {"B", "S"}


def emit_json(
    model: DataModel,
    client_output: Path,
    server_output: Path,
    stamp: str | None = None,
    prefix: str = "Dt",
    asset_name: str = "DA_{table}",
) -> None:
    client_output.mkdir(parents=True, exist_ok=True)
    server_output.mkdir(parents=True, exist_ok=True)
    enums = {enum.name: enum for enum in model.enums}
    content_hashes: dict[tuple[str, str], str] = {}
    for table in model.tables:
        rows = _sorted_rows(table, enums)
        for side, scopes, output, with_index in (
            ("client", CLIENT_SCOPES, client_output, True),
            ("server", SERVER_SCOPES, server_output, False),
        ):
            payload = _table_payload(table, rows, scopes, enums, with_index)
            content_hashes[(side, table.name)] = payload["content_hash"]
            _write_json(output / f"{table.name}.json", payload)
    for side, scopes, output in (
        ("client", CLIENT_SCOPES, client_output),
        ("server", SERVER_SCOPES, server_output),
    ):
        references = []
        for table in model.tables:
            for column in table.columns:
                if column.ref_target is None or column.scope not in scopes:
                    continue
                references.append({
                    "table": table.name,
                    "field": column.name,
                    "target": column.ref_target,
                    "target_key": column.ref_key,
                    "cardinality": "many" if column.ref_key else "one",
                    "key_type": column.type_name,
                    "array_length": column.array_size or 1,
                    "subkey": column.role == "subkey",
                })
        references.sort(key=lambda item: (item["table"], item["field"]))
        manifest: dict[str, object] = {
            "source_files": list(model.source_files),
            # 굽기 도구가 C++ 클래스 이름(U<prefix><Table>Table)과 에셋 이름을 같은 규칙으로 만들게 한다.
            "cpp_prefix": prefix,
            "asset_name": asset_name,
            "tables": [
                {
                    "name": table.name,
                    "rows": len(table.rows),
                    "schema_hash": table.schema_hash,
                    "content_hash": content_hashes[(side, table.name)],
                }
                for table in model.tables
            ],
            "enums": [{"name": enum.name, "values": len(enum.values)} for enum in model.enums],
            "references": references,
        }
        if stamp is not None:
            manifest["generated_at"] = stamp
        _write_json(output / "manifest.json", manifest)


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
    # 내용 해시: 이 산출물에 실제로 실린 데이터(행·인덱스)만으로 계산한다. 값만 바꾸고
    # 다시 굽지 않은 경우를 잡는 데 쓴다. 스키마 해시와 달리 값이 바뀌면 바뀐다.
    content = {key: value for key, value in payload.items() if key not in {"table", "schema_hash"}}
    digest = hashlib.sha256(
        json.dumps(content, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()
    ).hexdigest()
    ordered = {"table": payload["table"], "schema_hash": payload["schema_hash"],
               "content_hash": f"sha256:{digest}"}
    ordered.update(content)
    return ordered


def content_hash(table: TableSchema, enums: dict[str, EnumSchema], scopes: set[str] | None = None) -> str:
    """클라(기본) 또는 지정 범위 산출물의 내용 해시. C++ 기대값 상수에 쓴다."""
    target = scopes or CLIENT_SCOPES
    rows = _sorted_rows(table, enums)
    return _table_payload(table, rows, target, enums, target == CLIENT_SCOPES)["content_hash"]


def _write_json(path: Path, payload: object) -> None:
    text = json.dumps(payload, ensure_ascii=False, indent=2) + "\n"
    path.write_text(text, encoding="utf-8", newline="\n")
