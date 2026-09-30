"""런타임 연동 산출물(행 조회·참조 함수, 테이블별 .cpp, 등록 헤더) 테스트."""

from __future__ import annotations

from pathlib import Path

import pytest
from harness import main
from openpyxl import Workbook

RUNTIME = "TableData/DrTableRuntime.h"


def _sheet(workbook: Workbook, name: str, rows: list[list[object]]) -> None:
    sheet = workbook.create_sheet(name)
    for record in rows:
        sheet.append(record)


def _source(path: Path) -> None:
    workbook = Workbook()
    workbook.remove(workbook.active)
    _sheet(workbook, "<enum>Kind", [["Id", "Value"], ["ID<name>", "int32"], ["all", "all"],
                                    ["A", 0], ["B", 1]])
    _sheet(workbook, "Items", [
        ["Id", "Kind", "Quest", "ServerQuest"],
        ["ID<int32>", "SubKey<EKind>", "Ref<Quests>", "Ref<Quests>"],
        ["all", "all", "all", "server"],
        [1001, "A", 1, 1],
    ])
    _sheet(workbook, "Quests", [
        ["Id", "Reward", "Next[0]", "Next[1]"],
        ["ID<int32>", "SubKey<Ref<Items>>", "Ref<Quests>", "Ref<Quests>"],
        ["all", "all", "all", "all"],
        [1, 1001, 2, None], [2, None, None, None],
    ])
    _sheet(workbook, "DropTable", [
        ["Id", "GroupId", "KindKey"],
        ["ID<int32>", "SubKey<int32>", "SubKey<EKind>"],
        ["all", "all", "all"],
        [1, 10, "A"],
    ])
    _sheet(workbook, "Monsters", [
        ["Id", "DropGroup", "ByKind", "Name"],
        ["ID<name>", "Ref<DropTable.GroupId>", "Ref<DropTable.KindKey>", "Ref<Monsters>"],
        ["all", "all", "all", "all"],
        ["Wolf", 10, "A", None],
    ])
    workbook.save(path)


def _build(source: Path, root: Path, *extra: str) -> int:
    return main(["build", "--input", str(source), "--out-cpp", str(root / "cpp"),
                 "--out-client", str(root / "client"), "--out-server", str(root / "server"),
                 *extra])


def _read(root: Path, name: str) -> str:
    return (root / "cpp" / name).read_text(encoding="utf-8")


def test_without_runtime_header_no_accessors_are_generated(tmp_path: Path) -> None:
    source = tmp_path / "in.xlsx"
    _source(source)
    assert _build(source, tmp_path) == 0
    names = sorted(path.name for path in (tmp_path / "cpp").iterdir())
    assert not any(name.endswith(".cpp") for name in names)
    assert "DrTableRegistration.h" not in names
    row = _read(tmp_path, "DrQuestsRow.h")
    assert "static" not in row
    assert "struct FDrItemsRow;" not in row


def test_row_header_declarations_and_forward_declarations(tmp_path: Path) -> None:
    source = tmp_path / "in.xlsx"
    _source(source)
    assert _build(source, tmp_path, "--runtime-header", RUNTIME) == 0
    quests = _read(tmp_path, "DrQuestsRow.h")
    assert "struct FDrItemsRow;" in quests
    assert "struct FDrQuestsRow;" not in quests  # 자기 참조는 전방 선언하지 않는다.
    assert "#include \"DrItemsRow.h\"" not in quests  # 순환 include 금지
    assert "static const FDrQuestsRow* Find(int32 Key);" in quests
    assert "static TArray<const FDrQuestsRow*> FindByReward(int32 Key);" in quests
    assert "static TConstArrayView<FDrQuestsRow> GetAll();" in quests
    assert "const FDrItemsRow* GetReward() const;" in quests
    assert "const FDrQuestsRow* GetNext(int32 Index) const;" in quests

    items = _read(tmp_path, "DrItemsRow.h")
    assert "static TArray<const FDrItemsRow*> FindByKind(EDrKind Key);" in items
    assert "GetServerQuest" not in items  # 서버 전용 필드는 클라 구조체에 없다.

    monsters = _read(tmp_path, "DrMonstersRow.h")
    assert "static const FDrMonstersRow* Find(FName Key);" in monsters
    assert "TArray<const FDrDropTableRow*> GetDropGroup() const;" in monsters
    assert "TArray<const FDrDropTableRow*> GetByKind() const;" in monsters
    assert "const FDrMonstersRow* GetName() const;" in monsters


def test_row_source_definitions(tmp_path: Path) -> None:
    source = tmp_path / "in.xlsx"
    _source(source)
    assert _build(source, tmp_path, "--runtime-header", RUNTIME) == 0
    quests = _read(tmp_path, "DrQuestsRow.cpp")
    assert quests.splitlines()[1:4] == [
        '#include "DrQuestsRow.h"',
        '#include "DrItemsRow.h"',
        f'#include "{RUNTIME}"',
    ]
    assert "return DrTableRuntime::FindByKey<FDrQuestsRow>(Key);" in quests
    assert ('return DrTableRuntime::FindAllBySubKey<FDrQuestsRow>'
            '(FName(TEXT("Reward")), Key);') in quests
    assert "return DrTableRuntime::GetAll<FDrQuestsRow>();" in quests
    assert "    if (Index < 0 || Index >= 2 || Next[Index] == 0)" in quests
    assert "    return FDrQuestsRow::Find(Next[Index]);" in quests

    monsters = _read(tmp_path, "DrMonstersRow.cpp")
    assert "    if (DropGroup == 0)\n    {\n        return {};\n    }" in monsters
    assert "    return FDrDropTableRow::FindByGroupId(DropGroup);" in monsters
    assert "    return FDrDropTableRow::FindByKindKey(ByKind);" in monsters
    assert "    if (Name.IsNone())\n    {\n        return nullptr;\n    }" in monsters
    # 열거형 서브키 대상은 빈 셀이 금지라 없음 값 검사가 없다.
    by_kind = monsters.split("GetByKind() const\n{\n", 1)[1].split("}\n", 1)[0]
    assert "if (" not in by_kind


def test_registration_header(tmp_path: Path) -> None:
    source = tmp_path / "in.xlsx"
    _source(source)
    assert _build(source, tmp_path, "--runtime-header", RUNTIME,
                  "--asset-name", "BT_{table}") == 0
    text = _read(tmp_path, "DrTableRegistration.h")
    assert '#include "DrGeneratedTables.h"' in text
    assert '#include "DrQuestsTable.h"' in text
    assert "namespace DrGeneratedTables" in text
    assert "    template <typename TRegistry>\n    void RegisterAll(TRegistry& Registry)" in text
    assert ('Registry.template Register<FDrQuestsRow, UDrQuestsTable>(TEXT("BT_Quests"), '
            "&UDrQuestsTable::Rows, &UDrQuestsTable::PrimaryKeys)") in text
    assert ('            .WithSchemaHash(QuestsSchemaHash)\n'
            '            .WithSubKey(TEXT("Reward"), &UDrQuestsTable::Reward_Keys, '
            "&UDrQuestsTable::Reward_Offsets, &UDrQuestsTable::Reward_Indices);") in text
    assert '            .WithSchemaHash(MonstersSchemaHash);' in text
    assert "ContentHash" not in text
    # 테이블 이름 순서(결정성)
    order = [text.index(f"Register<FDt{name}Row") for name in
             ("DropTable", "Items", "KindInfo", "Monsters", "Quests") if f"Register<FDt{name}Row" in text]
    assert order == sorted(order)


def test_asset_name_requires_table_placeholder(tmp_path: Path,
                                               capsys: pytest.CaptureFixture[str]) -> None:
    source = tmp_path / "in.xlsx"
    _source(source)
    with pytest.raises(SystemExit) as raised:
        _build(source, tmp_path, "--runtime-header", RUNTIME, "--asset-name", "DA_Fixed")
    assert raised.value.code == 2
    assert "{table}" in capsys.readouterr().err


def test_runtime_outputs_are_deterministic(tmp_path: Path) -> None:
    source = tmp_path / "in.xlsx"
    _source(source)
    snapshots = []
    for run in ("a", "b"):
        root = tmp_path / run
        assert _build(source, root, "--runtime-header", RUNTIME) == 0
        snapshots.append({path.name: path.read_bytes() for path in (root / "cpp").iterdir()})
    assert snapshots[0] == snapshots[1]
    assert all(b"\r\n" not in content for content in snapshots[0].values())


@pytest.mark.parametrize(
    ("headers", "types", "message"),
    [
        (["Id", "Find"], ["ID<int32>", "int32"], "'Find'이 같은 이름의 필드"),
        (["Id", "GetAll"], ["ID<int32>", "int32"], "'GetAll'이 같은 이름의 필드"),
        (["Id", "Other", "GetOther"], ["ID<int32>", "Ref<T>", "int32"], "'GetOther'이 같은 이름의 필드"),
        (["Id", "Group", "FindByGroup"], ["ID<int32>", "SubKey<int32>", "int32"],
         "'FindByGroup'이 같은 이름의 필드"),
    ],
)
def test_member_name_collisions_are_errors(
    tmp_path: Path, capsys: pytest.CaptureFixture[str],
    headers: list[str], types: list[str], message: str,
) -> None:
    workbook = Workbook()
    workbook.remove(workbook.active)
    _sheet(workbook, "T", [headers, types, ["all"] * len(headers), [1] + [None] * (len(headers) - 1)])
    source = tmp_path / "in.xlsx"
    workbook.save(source)
    assert _build(source, tmp_path, "--runtime-header", RUNTIME) == 1
    error = capsys.readouterr().err
    assert message in error
    assert error.startswith("[T.schema.xlsx]T!")
    assert not (tmp_path / "cpp").exists()  # 오류면 아무것도 쓰지 않는다.


def test_member_name_collision_is_ignored_without_runtime_header(tmp_path: Path) -> None:
    workbook = Workbook()
    workbook.remove(workbook.active)
    _sheet(workbook, "T", [["Id", "Find"], ["ID<int32>", "int32"], ["all", "all"], [1, 2]])
    source = tmp_path / "in.xlsx"
    workbook.save(source)
    assert _build(source, tmp_path) == 0
