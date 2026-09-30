"""참고 헤더(데이터 시트 2·3행 수식)와 링크 경고 테스트. 엑셀 계산 결과는 PC 엑셀로 따로 확인했다.

도구는 기존 데이터 파일을 고치지 않는다. 참고 수식은 drtable new가 만드는 새 파일에만 넣는다.
"""

from __future__ import annotations

import zipfile
from pathlib import Path

import pytest
from harness import Schema, main, render_xlsx
from openpyxl import load_workbook

FIELDS = [("Id", "ID<int32>", "all"), ("Cost", "int32", "server")]


def _schemas(tmp_path: Path) -> Path:
    folder = tmp_path / "Tables" / "Schemas"
    folder.mkdir(parents=True)
    (folder / "Items.schema.xlsx").write_bytes(render_xlsx(Schema("", "Items", [(0, *f, None) for f in FIELDS])))
    return folder


def _new(tmp_path: Path, out: Path) -> int:
    return main(["new", "--table", "Items", "--out", str(out), "--schema", str(tmp_path / "Tables" / "Schemas")])


def _build(tmp_path: Path) -> int:
    out = tmp_path / "out"
    return main(["build", "--input", str(tmp_path / "Tables"), "--schema", str(tmp_path / "Tables" / "Schemas"),
                 "--out-cpp", str(out / "cpp"), "--out-client", str(out / "c"), "--out-server", str(out / "s")])


def _link_targets(path: Path) -> list[str]:
    with zipfile.ZipFile(path) as archive:
        return [archive.read(name).decode() for name in archive.namelist()
                if name.endswith(".xml.rels") and "externalLink" in name]


def test_new_creates_a_workbook_with_reference_formulas(tmp_path: Path) -> None:
    _schemas(tmp_path)
    out = tmp_path / "Tables" / "Items.xlsx"
    assert _new(tmp_path, out) == 0
    sheet = load_workbook(out).active
    assert sheet.title == "Items"
    assert [cell.value for cell in sheet[1]] == ["Id", "Cost"]
    assert sheet["A2"].value == (
        "=IFERROR(INDEX('[1]Items'!$B:$B,MATCH(A$1,'[1]Items'!$A:$A,0)),\"(스키마에 없음)\")"
    )
    assert sheet["B3"].value.startswith("=IFERROR(INDEX('[1]Items'!$C:$C,MATCH(B$1,")
    assert 'Target="Schemas/Items.schema.xlsx"' in _link_targets(out)[0]
    # 빌드는 2·3행을 읽지 않으니 빈 테이블로 통과한다.
    assert _build(tmp_path) == 0


def test_new_never_overwrites(tmp_path: Path, capsys: pytest.CaptureFixture[str]) -> None:
    _schemas(tmp_path)
    out = tmp_path / "Tables" / "Items.xlsx"
    out.write_bytes(b"designer data")
    assert _new(tmp_path, out) == 1
    assert "기존 데이터 파일을 고치지 않습니다" in capsys.readouterr().err
    assert out.read_bytes() == b"designer data"


def test_new_needs_a_schema(tmp_path: Path, capsys: pytest.CaptureFixture[str]) -> None:
    _schemas(tmp_path)
    assert main(["new", "--table", "Nope", "--out", str(tmp_path / "Tables" / "Nope.xlsx"),
                 "--schema", str(tmp_path / "Tables" / "Schemas")]) == 1
    assert "'Nope'의 스키마가 없습니다" in capsys.readouterr().err


def test_build_warns_when_the_link_path_is_missing_here(
    tmp_path: Path, capsys: pytest.CaptureFixture[str],
) -> None:
    _schemas(tmp_path)
    path = tmp_path / "Tables" / "Items.xlsx"
    assert _new(tmp_path, path) == 0
    # 다른 PC에서 저장한 것처럼 링크를 이 PC에 없는 절대 경로로 바꾼다.
    with zipfile.ZipFile(path) as archive:
        parts = {name: archive.read(name) for name in archive.namelist()}
    for name, data in parts.items():
        if "externalLink" in name and name.endswith(".rels"):
            parts[name] = data.replace(b'Target="Schemas/Items.schema.xlsx"',
                                       b'Target="file:///D:/Other/Schemas/Items.schema.xlsx"')
    with zipfile.ZipFile(path, "w") as archive:
        for name, data in parts.items():
            archive.writestr(name, data)
    capsys.readouterr()
    assert _build(tmp_path) == 0
    assert "참고 헤더가 이 PC에 없는 경로를 가리킵니다(file:///D:/Other/Schemas/Items.schema.xlsx)" in capsys.readouterr().err
    # 링크가 제대로 된 파일은 경고가 없다.
    path.unlink()
    assert _new(tmp_path, path) == 0
    capsys.readouterr()
    assert _build(tmp_path) == 0
    assert "참고 헤더" not in capsys.readouterr().err
