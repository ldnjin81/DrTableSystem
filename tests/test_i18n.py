"""Message language selection and English-only generated output."""

from __future__ import annotations

import re
from pathlib import Path

import pytest
from harness import main, set_language
from openpyxl import Workbook

HANGUL = re.compile("[가-힣]")


def _save(path: Path, rows: list[list[object]]) -> None:
    workbook = Workbook()
    sheet = workbook.active
    sheet.title = "Items"
    for record in rows:
        sheet.append(record)
    workbook.save(path)


def test_lang_en_prints_english_errors(tmp_path: Path, capsys: pytest.CaptureFixture[str]) -> None:
    source = tmp_path / "in.xlsx"
    _save(source, [["Id"], ["ID<int32>"], ["all"], [1], [1]])
    code = main(["--lang", "en", "build", "--input", str(source), "--out-cpp", str(tmp_path / "c"),
                 "--out-client", str(tmp_path / "cl"), "--out-server", str(tmp_path / "s")])
    assert code == 1
    error = capsys.readouterr().err
    assert "Items!A5: duplicate primary key '1'" in error
    assert not HANGUL.search(error)


def test_lang_ko_prints_korean_errors(tmp_path: Path, capsys: pytest.CaptureFixture[str]) -> None:
    source = tmp_path / "in.xlsx"
    _save(source, [["Id"], ["ID<int32>"], ["all"], [1], [1]])
    set_language("en")
    code = main(["--lang", "ko", "build", "--input", str(source), "--out-cpp", str(tmp_path / "c"),
                 "--out-client", str(tmp_path / "cl"), "--out-server", str(tmp_path / "s")])
    assert code == 1
    assert "기본키 값 '1'이 중복되었습니다" in capsys.readouterr().err


def test_generated_output_is_english(tmp_path: Path) -> None:
    source = tmp_path / "in.xlsx"
    _save(source, [["Id", "Name", "Next"], ["ID<int32>", "string", "Ref<Items>"], ["all", "all", "all"],
                   [1, "검", 0]])
    assert main(["build", "--input", str(source), "--out-cpp", str(tmp_path / "cpp"),
                 "--out-client", str(tmp_path / "client"), "--out-server", str(tmp_path / "server"),
                 "--ue-plugin"]) == 0
    assert main(["graph", "--input", str(source), "--out", str(tmp_path / "graph.md")]) == 0
    for path in [*(tmp_path / "cpp").iterdir(), tmp_path / "graph.md"]:
        assert not HANGUL.search(path.read_text(encoding="utf-8")), path.name


def test_folder_input_skips_excel_lock_files(tmp_path: Path) -> None:
    folder = tmp_path / "in"
    folder.mkdir()
    _save(folder / "Data.xlsx", [["Id"], ["ID<int32>"], ["all"], [1]])
    (folder / "~$Data.xlsx").write_bytes(b"lock")  # what Excel leaves while the book is open
    assert main(["build", "--input", str(folder), "--out-cpp", str(tmp_path / "c"),
                 "--out-client", str(tmp_path / "cl"), "--out-server", str(tmp_path / "s")]) == 0


def test_scope_words_are_case_insensitive(tmp_path: Path) -> None:
    source = tmp_path / "in.xlsx"
    _save(source, [["Id", "A", "B", "Note"], ["ID<int32>", "int32", "int32", "string"],
                   ["All", "Client", "SERVER", "#"], [1, 2, 3, "memo"]])
    assert main(["check", "--input", str(source)]) == 0
