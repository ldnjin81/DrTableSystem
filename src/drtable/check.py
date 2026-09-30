"""Checks the referential integrity of generated JSON (``drtable check``).

Reads only the JSON and manifest written by ``drtable build``; the spreadsheet is not
needed, so this can run in CI on the build output.
"""

from __future__ import annotations

import json
from pathlib import Path

from .i18n import tr


class CheckInputError(Exception):
    """The check input (manifest or table JSON) is malformed."""


def _read_json(path: Path) -> dict:
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as exc:
        raise CheckInputError(tr(
            f"{path.name}!A1: JSON을 읽을 수 없습니다: {exc}",
            f"{path.name}!A1: cannot read JSON: {exc}",
        )) from exc
    if not isinstance(data, dict):
        raise CheckInputError(tr(
            f"{path.name}!A1: JSON 최상위 값은 객체여야 합니다",
            f"{path.name}!A1: the top-level JSON value must be an object",
        ))
    return data


def _input_error(file: str, ko: str, en: str) -> CheckInputError:
    return CheckInputError(f"{file}!A1: " + tr(ko, en))


def check_directory(directory: Path) -> tuple[list[str], list[str]]:
    """Returns (failures, warnings) for one output directory (client or server)."""
    manifest = _read_json(directory / "manifest.json")
    tables = manifest.get("tables")
    references = manifest.get("references")
    if not isinstance(tables, list) or not isinstance(references, list):
        raise _input_error("manifest.json", "tables와 references 배열이 필요합니다",
                           "'tables' and 'references' arrays are required")
    names = set()
    payloads = {}
    for entry in tables:
        if not isinstance(entry, dict) or not isinstance(entry.get("name"), str):
            raise _input_error("manifest.json", "잘못된 tables 항목입니다", "invalid 'tables' entry")
        name = entry["name"]
        if name in names:
            raise _input_error("manifest.json", f"테이블 '{name}'이 중복되었습니다",
                               f"table '{name}' appears twice")
        names.add(name)
        payload = _read_json(directory / f"{name}.json")
        if payload.get("table") != name or not isinstance(payload.get("primary_key"), str):
            raise _input_error(f"{name}.json", "테이블 이름 또는 기본키가 잘못되었습니다",
                               "wrong table name or primary key")
        if not isinstance(payload.get("rows"), list):
            raise _input_error(f"{name}.json", "rows 배열이 필요합니다", "a 'rows' array is required")
        payloads[name] = payload

    keys = {}
    for name, payload in payloads.items():
        primary = payload["primary_key"]
        if any(not isinstance(row, dict) or primary not in row for row in payload["rows"]):
            raise _input_error(f"{name}.json", "기본키가 빠진 행이 있습니다",
                               "a row has no primary key")
        if any(
            isinstance(row[primary], bool) or not isinstance(row[primary], (str, int))
            for row in payload["rows"]
        ):
            raise _input_error(f"{name}.json", "기본키 자료형이 잘못되었습니다",
                               "wrong primary key type")
        keys[name] = {row[primary] for row in payload["rows"]}

    failures = []
    warnings = []
    for ref in references:
        if not isinstance(ref, dict) or any(
            not isinstance(ref.get(field), str)
            for field in ("table", "field", "target", "key_type")
        ):
            raise _input_error("manifest.json", "잘못된 references 항목입니다",
                               "invalid 'references' entry")
        source, field, target, key_type = (
            ref["table"], ref["field"], ref["target"], ref["key_type"]
        )
        if source not in payloads or target not in payloads:
            raise _input_error("manifest.json", f"참조 '{source}.{field}'의 테이블이 없습니다",
                               f"a table of reference '{source}.{field}' is missing")
        array_length = ref.get("array_length", 1)
        if isinstance(array_length, bool) or not isinstance(array_length, int) or array_length < 1:
            raise _input_error("manifest.json", "array_length가 잘못되었습니다",
                               "invalid array_length")
        target_key = ref.get("target_key")
        cardinality = ref.get("cardinality")
        if cardinality not in ("one", "many") or (target_key is None) != (cardinality == "one"):
            raise _input_error("manifest.json", "cardinality가 target_key와 맞지 않습니다",
                               "cardinality does not match target_key")
        if target_key is not None:
            if not isinstance(target_key, str) or not target_key:
                raise _input_error("manifest.json", "target_key가 잘못되었습니다", "invalid target_key")
            sub_keys = payloads[target].get("sub_keys")
            if not isinstance(sub_keys, list) or not any(
                isinstance(entry, dict) and entry.get("field") == target_key
                for entry in sub_keys
            ):
                raise _input_error(f"{target}.json", f"'{target_key}' 서브키 인덱스가 없습니다",
                                   f"no sub key index for '{target_key}'")
            if any(target_key not in row for row in payloads[target]["rows"]):
                raise _input_error(f"{target}.json", f"서브키 '{target_key}'가 빠진 행이 있습니다",
                                   f"a row has no sub key '{target_key}'")
            if any(
                isinstance(row[target_key], bool) or
                not isinstance(row[target_key], (str, int))
                for row in payloads[target]["rows"]
            ):
                raise _input_error(f"{target}.json", f"서브키 '{target_key}'의 자료형이 잘못되었습니다",
                                   f"wrong type for sub key '{target_key}'")
            target_keys = {row[target_key] for row in payloads[target]["rows"]}
            target_label = f"{target}.{target_key}"
        else:
            target_keys = keys[target]
            target_label = target
        # Empty cells of a reference column mean "no reference": 0 for numbers, "" for names.
        absent = "" if key_type == "name" else 0 if key_type in {"int32", "int64"} else None
        if absent is not None and absent in target_keys:
            warning = tr(
                f"{target_label}: 키 {absent!r}이 참조 없음 값과 충돌합니다",
                f"{target_label}: key {absent!r} collides with the 'no reference' value",
            )
            if warning not in warnings:
                warnings.append(warning)
        primary = payloads[source]["primary_key"]
        for row in payloads[source]["rows"]:
            if field not in row:
                raise _input_error(f"{source}.json", f"필드 '{field}'가 없습니다",
                                   f"field '{field}' is missing")
            value = row[field]
            if array_length != 1 or isinstance(value, list):
                if not isinstance(value, list) or len(value) != array_length:
                    raise _input_error(f"{source}.json", f"필드 '{field}'의 배열 길이가 다릅니다",
                                       f"field '{field}' has a different array length")
                values = enumerate(value)
            else:
                values = [(None, value)]
            for index, item in values:
                if isinstance(item, bool) or not isinstance(item, (str, int)):
                    raise _input_error(f"{source}.json", f"필드 '{field}'의 값 자료형이 잘못되었습니다",
                                       f"wrong value type in field '{field}'")
                if item == absent:
                    continue
                if item not in target_keys:
                    suffix = f"({index})" if index is not None else ""
                    # A sub key reference points at a group of rows, so report "no row has it".
                    if target_key:
                        missing = tr("에 해당 값 없음", ": no row has this value")
                    else:
                        missing = tr(" 테이블에 없음", ": not in the table")
                    failures.append(
                        f"{source}.{field}[{row[primary]}]{suffix} = {item} → {target_label}{missing}"
                    )
    return failures, warnings
