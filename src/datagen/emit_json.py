"""클라이언트·서버 JSON 생성."""

from __future__ import annotations

import json
from pathlib import Path

from .excel import DataModel
from .schema import TableSchema


def emit_json(
    model: DataModel,
    client_output: Path,
    server_output: Path,
    stamp: str | None = None,
) -> None:
    client_output.mkdir(parents=True, exist_ok=True)
    server_output.mkdir(parents=True, exist_ok=True)
    for table in model.tables:
        _write_json(client_output / f"{table.name}.json", _table_payload(table, {"B", "C"}))
        _write_json(server_output / f"{table.name}.json", _table_payload(table, {"B", "S"}))
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


def _table_payload(table: TableSchema, scopes: set[str]) -> dict[str, object]:
    included = [column for column in table.columns if column.scope in scopes]
    return {
        "table": table.name,
        "schema_hash": table.schema_hash,
        "primary_key": table.primary_key.name,
        "sub_keys": [
            {"name": column.name, "field": column.name}
            for column in table.sub_keys
            if column.scope in scopes
        ],
        "rows": [
            {column.name: row[column.name] for column in included}
            for row in table.rows
        ],
    }


def _write_json(path: Path, payload: object) -> None:
    text = json.dumps(payload, ensure_ascii=False, indent=2) + "\n"
    path.write_text(text, encoding="utf-8", newline="\n")
