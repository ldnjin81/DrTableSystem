"""Converts cell values to schema types."""

from __future__ import annotations

import math
import re

from .errors import ErrorCollector
from .i18n import tr
from .schema import EnumSchema

INTEGER_TYPES = {"int32": (-(2**31), 2**31 - 1), "int64": (-(2**63), 2**63 - 1)}
ENUM_RE = re.compile(r"^E(?P<name>[A-Za-z][A-Za-z0-9_]*)$")


def convert_value(
    value: object,
    type_name: str,
    enums: dict[str, EnumSchema],
    sheet: str,
    cell: str,
    errors: ErrorCollector,
    *,
    use_default_for_empty: bool = True,
) -> object:
    if use_default_for_empty and (value is None or value == ""):
        return default_value(type_name, enums)
    try:
        if type_name in INTEGER_TYPES:
            if isinstance(value, bool):
                raise ValueError
            number = int(value)
            if isinstance(value, float) and not value.is_integer():
                raise ValueError
            if isinstance(value, str) and not re.fullmatch(r"[+-]?\d+", value.strip()):
                raise ValueError
            minimum, maximum = INTEGER_TYPES[type_name]
            if not minimum <= number <= maximum:
                raise ValueError
            return number
        if type_name in {"float", "double"}:
            if isinstance(value, bool):
                raise ValueError
            number = float(value)
            if not math.isfinite(number):
                raise ValueError
            return number
        if type_name == "bool":
            if isinstance(value, bool):
                return value
            if value in (0, 1):
                return bool(value)
            if isinstance(value, str):
                normalized = value.strip().lower()
                if normalized in {"true", "1"}:
                    return True
                if normalized in {"false", "0"}:
                    return False
            raise ValueError
        if type_name in {"name", "string", "text", "tag", "path"}:
            if not isinstance(value, str):
                raise ValueError
            return value
        enum_match = ENUM_RE.fullmatch(type_name)
        if enum_match:
            enum = enums.get(enum_match.group("name"))
            if enum is None:
                return ""
            text = str(value)
            if text not in {item.name for item in enum.values}:
                raise ValueError
            return text
    except (TypeError, ValueError, OverflowError):
        errors.add(sheet, cell, tr(
            f"'{value}' 값을 {type_name} 자료형으로 변환할 수 없습니다",
            f"cannot convert '{value}' to {type_name}",
        ))
        return default_value(type_name, enums)
    errors.add(sheet, cell, tr(f"지원하지 않는 자료형 '{type_name}'", f"unsupported type '{type_name}'"))
    return None


def default_value(type_name: str, enums: dict[str, EnumSchema]) -> object:
    if type_name in {"int32", "int64", "float", "double"}:
        return 0
    if type_name == "bool":
        return False
    if type_name in {"name", "string", "text", "tag", "path"}:
        return ""
    match = ENUM_RE.fullmatch(type_name)
    if match and match.group("name") in enums and enums[match.group("name")].values:
        return enums[match.group("name")].values[0].name
    return ""
