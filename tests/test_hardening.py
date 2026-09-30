"""2026-09-30 검수에서 나온 결함 수정의 회귀 테스트."""

from __future__ import annotations

import json
from pathlib import Path

import pytest
from openpyxl import Workbook

from tablegen.cli import main


def _book(sheets: dict[str, list[list[object]]]) -> Workbook:
    workbook = Workbook()
    workbook.remove(workbook.active)
    for name, rows in sheets.items():
        sheet = workbook.create_sheet(name)
        for record in rows:
            sheet.append(record)
    return workbook


def _build(source: Path, root: Path, *extra: str) -> int:
    return main(["build", "--input", str(source), "--out-cpp", str(root / "cpp"),
                 "--out-client", str(root / "client"), "--out-server", str(root / "server"), *extra])


def _enum_sheet(values: list[tuple[str, int]]) -> list[list[object]]:
    return [["all", "all"], ["ID<name>", "int32"], ["Id", "Value"], *[[n, v] for n, v in values]]


def _schema_hash(root: Path, table: str) -> str:
    return json.loads((root / "client" / f"{table}.json").read_text(encoding="utf-8"))["schema_hash"]


def test_enum_definition_changes_schema_hash(tmp_path: Path) -> None:
    table = [["all", "all"], ["ID<int32>", "SubKey<EKind>"], ["Id", "Kind"], [1, "A"], [2, "B"]]
    for run, values in (("a", [("A", 0), ("B", 1)]), ("b", [("A", 1), ("B", 0)])):
        source = tmp_path / f"{run}.xlsx"
        _book({"<enum>Kind": _enum_sheet(values), "T": table}).save(source)
        assert _build(source, tmp_path / run) == 0
    # 열거형 값만 바꿔도 구운 인덱스 정렬이 달라지므로 스키마 해시가 바뀌어야 한다.
    assert _schema_hash(tmp_path / "a", "T") != _schema_hash(tmp_path / "b", "T")


def test_unused_enum_does_not_change_schema_hash(tmp_path: Path) -> None:
    table = [["all"], ["ID<int32>"], ["Id"], [1]]
    hashes = []
    for run, values in (("a", [("A", 0)]), ("b", [("A", 0), ("Z", 9)])):
        source = tmp_path / f"{run}.xlsx"
        _book({"<enum>Other": _enum_sheet(values), "T": table}).save(source)
        assert _build(source, tmp_path / run) == 0
        hashes.append(_schema_hash(tmp_path / run, "T"))
    assert hashes[0] == hashes[1]


@pytest.mark.parametrize(
    ("types", "rows"),
    [
        (["ID<name>"], [["Sword"], ["sword"]]),
        (["ID<int32>", "SubKey<name>"], [[1, "Fire"], [2, "FIRE"]]),
    ],
)
def test_name_keys_differing_only_by_case_are_rejected(
    tmp_path: Path, capsys: pytest.CaptureFixture[str],
    types: list[str], rows: list[list[object]],
) -> None:
    headers = ["Id", "Group"][: len(types)]
    source = tmp_path / "in.xlsx"
    _book({"T": [["all"] * len(types), types, headers, *rows]}).save(source)
    assert _build(source, tmp_path) == 1
    error = capsys.readouterr().err
    assert "대소문자만 다릅니다" in error
    assert error.startswith("T!")


def test_same_subkey_value_repeated_is_fine(tmp_path: Path) -> None:
    source = tmp_path / "in.xlsx"
    _book({"T": [["all", "all"], ["ID<int32>", "SubKey<name>"], ["Id", "Group"],
                 [1, "Fire"], [2, "Fire"]]}).save(source)
    assert _build(source, tmp_path) == 0


def test_content_hash_tracks_values_and_manifest_records_naming(tmp_path: Path) -> None:
    hashes = []
    for run, value in (("a", 10), ("b", 11)):
        source = tmp_path / f"{run}.xlsx"
        _book({"T": [["all", "all"], ["ID<int32>", "int32"], ["Id", "Power"], [1, value]]}).save(source)
        assert _build(source, tmp_path / run, "--prefix", "Gm", "--asset-name", "BT_{table}") == 0
        payload = json.loads((tmp_path / run / "client" / "T.json").read_text(encoding="utf-8"))
        manifest = json.loads((tmp_path / run / "client" / "manifest.json").read_text(encoding="utf-8"))
        assert manifest["cpp_prefix"] == "Gm"
        assert manifest["asset_name"] == "BT_{table}"
        assert manifest["tables"][0]["content_hash"] == payload["content_hash"]
        header = (tmp_path / run / "cpp" / "GmGeneratedTables.h").read_text(encoding="utf-8")
        assert f'TContentHash[] = TEXT("{payload["content_hash"]}")' in header
        hashes.append((payload["schema_hash"], payload["content_hash"]))
    assert hashes[0][0] == hashes[1][0]  # 구조는 같다
    assert hashes[0][1] != hashes[1][1]  # 값이 바뀌면 내용 해시가 바뀐다


def test_ue_plugin_preset(tmp_path: Path) -> None:
    source = tmp_path / "in.xlsx"
    _book({"T": [["all"], ["ID<int32>"], ["Id"], [1]]}).save(source)
    assert _build(source, tmp_path, "--ue-plugin", "--prefix", "Gm") == 0
    table = (tmp_path / "cpp" / "GmTTable.h").read_text(encoding="utf-8")
    assert '#include "TableGenAssetBase.h"' in table
    assert "class UGmTTable : public UTableGenAssetBase" in table
    source_cpp = (tmp_path / "cpp" / "GmTRow.cpp").read_text(encoding="utf-8")
    assert '#include "TableGenRuntime.h"' in source_cpp
    assert (tmp_path / "cpp" / "GmTableRegistration.h").exists()
