"""생성된 JSON의 테이블 참조 무결성을 검사한다."""

from __future__ import annotations

import json
from pathlib import Path


class CheckInputError(Exception):
    """검사 입력이 잘못되었습니다."""


def _read_json(path: Path) -> dict:
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as exc:
        raise CheckInputError(f"{path.name}!A1: JSON을 읽을 수 없습니다: {exc}") from exc
    if not isinstance(data, dict):
        raise CheckInputError(f"{path.name}!A1: JSON 최상위 값은 객체여야 합니다")
    return data


def check_directory(directory: Path) -> tuple[list[str], list[str]]:
    manifest = _read_json(directory / "manifest.json")
    tables = manifest.get("tables")
    references = manifest.get("references")
    if not isinstance(tables, list) or not isinstance(references, list):
        raise CheckInputError("manifest.json!A1: tables와 references 배열이 필요합니다")
    names = set()
    payloads = {}
    for entry in tables:
        if not isinstance(entry, dict) or not isinstance(entry.get("name"), str):
            raise CheckInputError("manifest.json!A1: 잘못된 tables 항목입니다")
        name = entry["name"]
        if name in names:
            raise CheckInputError(f"manifest.json!A1: 테이블 '{name}'이 중복되었습니다")
        names.add(name)
        payload = _read_json(directory / f"{name}.json")
        if payload.get("table") != name or not isinstance(payload.get("primary_key"), str):
            raise CheckInputError(f"{name}.json!A1: 테이블 이름 또는 기본키가 잘못되었습니다")
        if not isinstance(payload.get("rows"), list):
            raise CheckInputError(f"{name}.json!A1: rows 배열이 필요합니다")
        payloads[name] = payload

    keys = {}
    for name, payload in payloads.items():
        primary = payload["primary_key"]
        if any(not isinstance(row, dict) or primary not in row for row in payload["rows"]):
            raise CheckInputError(f"{name}.json!A1: 기본키가 빠진 행이 있습니다")
        if any(
            isinstance(row[primary], bool) or not isinstance(row[primary], (str, int))
            for row in payload["rows"]
        ):
            raise CheckInputError(f"{name}.json!A1: 기본키 자료형이 잘못되었습니다")
        keys[name] = {row[primary] for row in payload["rows"]}

    failures = []
    warnings = []
    for ref in references:
        if not isinstance(ref, dict) or any(
            not isinstance(ref.get(field), str)
            for field in ("table", "field", "target", "key_type")
        ):
            raise CheckInputError("manifest.json!A1: 잘못된 references 항목입니다")
        source, field, target, key_type = (
            ref["table"], ref["field"], ref["target"], ref["key_type"]
        )
        if source not in payloads or target not in payloads:
            raise CheckInputError(
                f"manifest.json!A1: 참조 '{source}.{field}'의 테이블이 없습니다"
            )
        array_length = ref.get("array_length", 1)
        if isinstance(array_length, bool) or not isinstance(array_length, int) or array_length < 1:
            raise CheckInputError("manifest.json!A1: array_length가 잘못되었습니다")
        target_key = ref.get("target_key")
        cardinality = ref.get("cardinality")
        if cardinality not in ("one", "many") or (target_key is None) != (cardinality == "one"):
            raise CheckInputError("manifest.json!A1: cardinality가 target_key와 맞지 않습니다")
        if target_key is not None:
            if not isinstance(target_key, str) or not target_key:
                raise CheckInputError("manifest.json!A1: target_key가 잘못되었습니다")
            sub_keys = payloads[target].get("sub_keys")
            if not isinstance(sub_keys, list) or not any(
                isinstance(entry, dict) and entry.get("field") == target_key
                for entry in sub_keys
            ):
                raise CheckInputError(f"{target}.json!A1: '{target_key}' 서브키 인덱스가 없습니다")
            if any(target_key not in row for row in payloads[target]["rows"]):
                raise CheckInputError(f"{target}.json!A1: 서브키 '{target_key}'가 빠진 행이 있습니다")
            if any(
                isinstance(row[target_key], bool) or
                not isinstance(row[target_key], (str, int))
                for row in payloads[target]["rows"]
            ):
                raise CheckInputError(f"{target}.json!A1: 서브키 '{target_key}'의 자료형이 잘못되었습니다")
            target_keys = {row[target_key] for row in payloads[target]["rows"]}
            target_label = f"{target}.{target_key}"
        else:
            target_keys = keys[target]
            target_label = target
        absent = "" if key_type == "name" else 0 if key_type in {"int32", "int64"} else None
        if absent is not None and absent in target_keys:
            warning = f"{target_label}: 키 {absent!r}이 참조 없음 값과 충돌합니다"
            if warning not in warnings:
                warnings.append(warning)
        primary = payloads[source]["primary_key"]
        for row in payloads[source]["rows"]:
            if field not in row:
                raise CheckInputError(f"{source}.json!A1: 필드 '{field}'가 없습니다")
            value = row[field]
            if array_length != 1 or isinstance(value, list):
                if not isinstance(value, list) or len(value) != array_length:
                    raise CheckInputError(f"{source}.json!A1: 필드 '{field}'의 배열 길이가 다릅니다")
                values = enumerate(value)
            else:
                values = [(None, value)]
            for index, item in values:
                if isinstance(item, bool) or not isinstance(item, (str, int)):
                    raise CheckInputError(f"{source}.json!A1: 필드 '{field}'의 값 자료형이 잘못되었습니다")
                if item == absent:
                    continue
                if item not in target_keys:
                    suffix = f"({index})" if index is not None else ""
                    failures.append(
                        f"{source}.{field}[{row[primary]}]{suffix} = {item} → {target_label} 테이블에 없음"
                    )
    return failures, warnings

