"""참고 헤더(데이터 시트 2·3행 수식)와 링크 경고 테스트. 엑셀 계산 결과는 PC 엑셀로 따로 확인했다."""

from __future__ import annotations

import zipfile
from pathlib import Path

import pytest
from conftest import plain_save
from openpyxl import Workbook, load_workbook

from drtable.cli import main
from drtable.schemafile import Schema, render_xlsx

FIELDS = [("Id", "ID<int32>", "all"), ("Cost", "int32", "server")]


def _setup(tmp_path: Path, data_rel: str = "Items.xlsx", schema_dir: str = "Schemas") -> Path:
    folder = tmp_path / schema_dir
    folder.mkdir(parents=True, exist_ok=True)
    (folder / "Items.schema.xlsx").write_bytes(render_xlsx(Schema("", "Items", [(0, *f, None) for f in FIELDS])))
    workbook = Workbook()
    sheet = workbook.active
    sheet.title = "Items#무기"
    for row in (["Cost", "Id", "#메모", "Extra"], [], [], [10, 1, "x", 5]):
        sheet.append(row)
    path = tmp_path / "Data" / data_rel
    path.parent.mkdir(parents=True, exist_ok=True)
    plain_save(workbook, path)
    return path


def _headers(tmp_path: Path) -> int:
    return main(["headers", "--input", str(tmp_path / "Data"), "--schema", str(tmp_path / "Schemas")])


def _link_targets(path: Path) -> list[str]:
    with zipfile.ZipFile(path) as archive:
        return [archive.read(name).decode() for name in archive.namelist() if name.endswith(".xml.rels") and "externalLink" in name]


def test_headers_write_lookup_formulas(tmp_path: Path) -> None:
    path = _setup(tmp_path)
    assert _headers(tmp_path) == 0
    sheet = load_workbook(path).active
    assert sheet["A2"].value == (
        "=IFERROR(INDEX('[1]Items'!$B:$B,MATCH(A$1,'[1]Items'!$A:$A,0)),\"(스키마에 없음)\")"
    )
    assert sheet["B3"].value.startswith("=IFERROR(INDEX('[1]Items'!$C:$C,MATCH(B$1,")
    assert sheet["C2"].value is None  # '#' 메모 열은 건드리지 않는다
    assert sheet["D2"].value.startswith("=IFERROR(")  # 스키마에 없는 이름은 수식이 표시해 준다
    assert sheet["A4"].value == 10  # 데이터는 그대로
    assert 'Target="../Schemas/Items.schema.xlsx"' in _link_targets(path)[0]
    # 다시 실행해도 링크는 하나만 쓴다.
    assert _headers(tmp_path) == 0
    assert len(_link_targets(path)) == 1


def test_build_ignores_header_formulas(tmp_path: Path) -> None:
    path = _setup(tmp_path)
    workbook = load_workbook(path)
    workbook.active.delete_cols(4)  # 스키마에 없는 Extra 열 제거
    plain_save(workbook, path)
    assert _headers(tmp_path) == 0
    out = tmp_path / "out"
    assert main(["build", "--input", str(tmp_path / "Data"), "--schema", str(tmp_path / "Schemas"),
                 "--out-cpp", str(out / "cpp"), "--out-client", str(out / "c"), "--out-server", str(out / "s")]) == 0


def test_workbooks_with_pictures_are_left_alone(tmp_path: Path, capsys: pytest.CaptureFixture[str]) -> None:
    path = _setup(tmp_path)
    with zipfile.ZipFile(path, "a") as archive:
        archive.writestr("xl/media/image1.png", b"png")
    before = path.read_bytes()
    assert _headers(tmp_path) == 1
    assert "그림·차트 등(media)" in capsys.readouterr().err
    assert path.read_bytes() == before


def test_build_warns_when_the_link_path_is_missing_here(
    tmp_path: Path, capsys: pytest.CaptureFixture[str],
) -> None:
    path = _setup(tmp_path)
    assert _headers(tmp_path) == 0
    # 다른 PC에서 저장한 것처럼 링크를 이 PC에 없는 절대 경로로 바꾼다.
    with zipfile.ZipFile(path) as archive:
        parts = {name: archive.read(name) for name in archive.namelist()}
    for name, data in parts.items():
        if "externalLink" in name and name.endswith(".rels"):
            parts[name] = data.replace(b'Target="../Schemas/Items.schema.xlsx"',
                                       b'Target="file:///D:/Other/Schemas/Items.schema.xlsx"')
    with zipfile.ZipFile(path, "w") as archive:
        for name, data in parts.items():
            archive.writestr(name, data)
    fixed = load_workbook(path)
    fixed.active.delete_cols(4)  # Extra 열을 지워 빌드가 통과하게 한다
    plain_save(fixed, path)
    out = tmp_path / "out"
    capsys.readouterr()
    assert main(["build", "--input", str(tmp_path / "Data"), "--schema", str(tmp_path / "Schemas"),
                 "--out-cpp", str(out / "cpp"), "--out-client", str(out / "c"), "--out-server", str(out / "s")]) == 0
    assert "참고 헤더가 이 PC에 없는 경로를 가리킵니다(file:///D:/Other/Schemas/Items.schema.xlsx)" in capsys.readouterr().err
