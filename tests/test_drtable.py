from __future__ import annotations

import json
from pathlib import Path

import pytest
from openpyxl import Workbook

from drtable.cli import main


def add_enum(workbook: Workbook) -> None:
    sheet = workbook.active
    sheet.title = "<enum>Element"
    sheet.append(["Id", "Value", "Comment"])
    sheet.append(["ID<name>", "int32", "string"])
    sheet.append(["all", "all", "#"])
    sheet.append(["Fire", 0, "불"])
    sheet.append(["Water", None, "물"])


def add_table(workbook: Workbook, title: str = "Effects") -> object:
    sheet = workbook.create_sheet(title)
    sheet.append([
            "Id",
            "Name",
            "Element",
            "ClientOnly",
            "ServerOnly",
            "Memo",
            "Reward[1]",
            "Reward[0]",
        ])
    sheet.append(
        [
            "ID<int32>",
            "SubKey<name>",
            "SubKey<EElement>",
            "float",
            "int64",
            "설명",
            "int32",
            "int32",
        ]
    )
    sheet.append(
        ["all", "all", "all", "client", "server", "#", "all", "all"]
    )
    sheet.append([1001, "Burn", "Fire", 12.5, 99, "무시", 20, 10])
    sheet.append([1002, "Freeze", "Water", None, 100, "무시", None, 30])
    return sheet


def save_valid(path: Path) -> None:
    workbook = Workbook()
    add_enum(workbook)
    add_table(workbook)
    ignored = workbook.create_sheet("#메모")
    ignored["A1"] = "이 시트는 형식과 무관하게 무시된다"
    workbook.save(path)


def output_bytes(root: Path) -> dict[str, bytes]:
    return {
        str(path.relative_to(root)): path.read_bytes()
        for path in sorted(root.rglob("*"))
        if path.is_file()
    }


def test_build_outputs_scope_array_and_determinism(tmp_path: Path) -> None:
    source = tmp_path / "Tables.xlsx"
    save_valid(source)
    cpp = tmp_path / "cpp"
    client = tmp_path / "client"
    server = tmp_path / "server"
    args = [
        "build",
        "--input",
        str(source),
        "--out-cpp",
        str(cpp),
        "--out-client",
        str(client),
        "--out-server",
        str(server),
    ]
    assert main(args) == 0
    first = output_bytes(tmp_path)
    assert {path.name for path in cpp.iterdir()} == {
        "EDtElement.h",
        "DtEffectsRow.h",
        "DtEffectsTable.h",
        "DtGeneratedTables.h",
    }
    header = (cpp / "DtEffectsRow.h").read_text(encoding="utf-8")
    enum_header = (cpp / "EDtElement.h").read_text(encoding="utf-8")
    assert "Fire = 0, // 불" in enum_header
    assert "int32 Id = 0;" in header
    assert "FName Name;" in header
    assert "EDtElement Element = EDtElement::Fire;" in header
    assert "float ClientOnly = 0.0f;" in header
    assert "int32 Reward[2] = {};" in header
    assert "ClientOnly" in header
    assert "ServerOnly" not in header
    client_data = json.loads((client / "Effects.json").read_text(encoding="utf-8"))
    server_data = json.loads((server / "Effects.json").read_text(encoding="utf-8"))
    assert client_data["rows"][0]["Reward"] == [10, 20]
    assert client_data["rows"][1]["Reward"] == [30, 0]
    assert "ClientOnly" in client_data["rows"][0]
    assert "ServerOnly" not in client_data["rows"][0]
    assert "ServerOnly" in server_data["rows"][0]
    assert "ClientOnly" not in server_data["rows"][0]
    assert "Memo" not in client_data["rows"][0]
    assert [item["field"] for item in client_data["sub_keys"]] == ["Name", "Element"]
    assert main(args) == 0
    assert output_bytes(tmp_path) == first


def test_cpp_array_property_is_not_exposed_to_blueprint(tmp_path: Path) -> None:
    source = tmp_path / "Tables.xlsx"
    save_valid(source)
    output = tmp_path / "output"

    assert _build(source, output) == 0

    header = (output / "cpp" / "DtEffectsRow.h").read_text(encoding="utf-8")
    scalar_property = (
        '    UPROPERTY(EditAnywhere, BlueprintReadOnly, Category = "Dt|Effects")\n'
        "    int32 Id = 0;"
    )
    array_property = (
        '    UPROPERTY(EditAnywhere, Category = "Dt|Effects")\n'
        "    int32 Reward[2] = {};"
    )
    assert scalar_property in header
    assert array_property in header


def test_enum_info_table_and_stamp(tmp_path: Path) -> None:
    # 열거형 시트의 부가 열은 migrate가 ItemTypeInfo 테이블(스키마 + 새 데이터 파일)로 옮긴다.
    workbook = Workbook()
    sheet = workbook.active
    sheet.title = "<enum>ItemType"
    sheet.append(["Id", "Value", "Comment", "DisplayName", "MaxStack"])
    sheet.append(["ID<name>", "int32", "string", "string", "int32"])
    sheet.append(["all", "all", "#", "client", "server"])
    sheet.append(["Weapon", 0, "무기류", "무기", 1])
    source = tmp_path / "enum-info.xlsx"
    workbook.save(source)
    result = main(
        [
            "build",
            "--input",
            str(tmp_path),
            "--out-cpp",
            str(tmp_path / "cpp"),
            "--out-client",
            str(tmp_path / "client"),
            "--out-server",
            str(tmp_path / "server"),
            "--stamp",
            "2026-09-17T00:00:00Z",
        ]
    )
    assert result == 0
    row_header = (tmp_path / "cpp" / "DtItemTypeInfoRow.h").read_text(encoding="utf-8")
    assert "EDtItemType Id = EDtItemType::Weapon;" in row_header
    assert "Value" not in row_header
    assert "Comment" not in row_header
    client = json.loads((tmp_path / "client" / "ItemTypeInfo.json").read_text(encoding="utf-8"))
    server = json.loads((tmp_path / "server" / "ItemTypeInfo.json").read_text(encoding="utf-8"))
    assert client["rows"] == [{"Id": "Weapon", "DisplayName": "무기"}]
    assert server["rows"] == [{"Id": "Weapon", "MaxStack": 1}]
    manifest = json.loads((tmp_path / "client" / "manifest.json").read_text(encoding="utf-8"))
    assert manifest["generated_at"] == "2026-09-17T00:00:00Z"


def test_cpp_scalar_initializers(tmp_path: Path) -> None:
    workbook = Workbook()
    sheet = workbook.active
    sheet.title = "Numbers"
    sheet.append(["Id", "Big", "Ratio", "Precise", "Enabled", "Label"])
    sheet.append(["ID<int32>", "int64", "float", "double", "bool", "string"])
    sheet.append(["all", "all", "all", "all", "all", "all"])
    sheet.append([1, 2, 3.5, 4.5, True, "값"])
    source = tmp_path / "initializers.xlsx"
    workbook.save(source)

    assert _build(source, tmp_path) == 0
    header = (tmp_path / "cpp" / "DtNumbersRow.h").read_text(encoding="utf-8")
    assert "int32 Id = 0;" in header
    assert "int64 Big = 0;" in header
    assert "float Ratio = 0.0f;" in header
    assert "double Precise = 0.0;" in header
    assert "bool Enabled = false;" in header
    assert "FString Label;" in header


def test_column_defaults_apply_to_cpp_and_empty_cells_deterministically(
    tmp_path: Path,
) -> None:
    workbook = Workbook()
    add_enum(workbook)
    sheet = workbook.create_sheet("Defaults")
    sheet.append(
        [
            "Id",
            "Multiplier",
            "Count",
            "Enabled",
            "Label",
            "Name",
            "DisplayName",
            "StateTag",
            "Icon",
            "Element",
            "Reward[0]",
            "Reward[1]",
        ]
    )
    sheet.append(
        [
            "ID<int32>",
            "float=1.0",
            "int32=7",
            "bool=true",
            "string=기본값",
            "name=Fallback",
            "text=표시값",
            "tag=State.Default",
            "path=/Game/UI/T_Default.T_Default",
            "EElement=Water",
            "int32=10",
            "int32=20",
        ]
    )
    sheet.append(["all"] * 12)
    sheet.append([1] + [None] * 11)
    sheet.append(
        [
            2,
            2.5,
            3,
            False,
            "직접값",
            "Direct",
            "직접 표시",
            "State.Direct",
            "/Game/UI/T_Direct.T_Direct",
            "Fire",
            30,
            40,
        ]
    )
    source = tmp_path / "defaults.xlsx"
    workbook.save(source)

    output = tmp_path / "output"
    assert _build(source, output) == 0
    first = output_bytes(output)

    header = (output / "cpp" / "DtDefaultsRow.h").read_text(encoding="utf-8")
    assert "float Multiplier = 1.0f;" in header
    assert "int32 Count = 7;" in header
    assert "bool Enabled = true;" in header
    assert 'FString Label = FString(TEXT("기본값"));' in header
    assert 'FName Name = FName(TEXT("Fallback"));' in header
    assert 'FText DisplayName = FText::FromString(TEXT("표시값"));' in header
    assert (
        'FGameplayTag StateTag = FGameplayTag::RequestGameplayTag('
        'FName(TEXT("State.Default")), false);'
    ) in header
    assert (
        'FSoftObjectPath Icon = FSoftObjectPath('
        'TEXT("/Game/UI/T_Default.T_Default"));'
    ) in header
    assert "EDtElement Element = EDtElement::Water;" in header
    assert "int32 Reward[2] = {10, 20};" in header

    expected = {
        "Id": 1,
        "Multiplier": 1.0,
        "Count": 7,
        "Enabled": True,
        "Label": "기본값",
        "Name": "Fallback",
        "DisplayName": "표시값",
        "StateTag": "State.Default",
        "Icon": "/Game/UI/T_Default.T_Default",
        "Element": "Water",
        "Reward": [10, 20],
    }
    client = json.loads((output / "client" / "Defaults.json").read_text(encoding="utf-8"))
    server = json.loads((output / "server" / "Defaults.json").read_text(encoding="utf-8"))
    assert client["rows"][0] == expected
    assert server["rows"][0] == expected

    assert _build(source, output) == 0
    assert output_bytes(output) == first


@pytest.mark.parametrize("key_type", ["ID<int32>=1", "SubKey<name>=Fallback"])
def test_key_defaults_are_rejected_without_outputs(
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
    key_type: str,
) -> None:
    workbook = Workbook()
    sheet = workbook.active
    sheet.title = "KeyDefault"
    sheet.append(["Id", "Lookup"])
    sheet.append([key_type, "string"] if key_type.startswith("ID") else ["ID<int32>", key_type])
    sheet.append(["all", "all"])
    sheet.append([1, "값"])
    source = tmp_path / "key-default.xlsx"
    workbook.save(source)

    output = tmp_path / "output"
    assert _build(source, output) == 1
    stderr = capsys.readouterr().err
    expected_cell = "B2" if key_type.startswith("ID") else "B3"
    assert f"[KeyDefault.schema.xlsx]KeyDefault!{expected_cell}" in stderr
    assert "기본키와 서브키에는 기본값을 지정할 수 없습니다" in stderr
    assert not (output / "cpp").exists()
    assert not (output / "client").exists()
    assert not (output / "server").exists()


def test_invalid_column_default_is_rejected_without_outputs(
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    workbook = Workbook()
    sheet = workbook.active
    sheet.title = "InvalidDefault"
    sheet.append(["Id", "Multiplier"])
    sheet.append(["ID<int32>", "float=not-a-number"])
    sheet.append(["all", "all"])
    sheet.append([1, None])
    source = tmp_path / "invalid-default.xlsx"
    workbook.save(source)

    output = tmp_path / "output"
    assert _build(source, output) == 1
    stderr = capsys.readouterr().err
    assert "[InvalidDefault.schema.xlsx]InvalidDefault!B3" in stderr
    assert "float 자료형으로 변환할 수 없습니다" in stderr
    assert not (output / "cpp").exists()
    assert not (output / "client").exists()
    assert not (output / "server").exists()


def test_semantic_string_types_generate_cpp_json_and_conditional_includes(
    tmp_path: Path,
) -> None:
    workbook = Workbook()
    sheet = workbook.active
    sheet.title = "Types"
    sheet.append(
        ["Id", "Label", "DisplayName", "StateTag", "Icon", "Count", "Enabled"]
    )
    sheet.append(
        ["ID<name>", "string", "text", "tag", "path", "int32", "bool"]
    )
    sheet.append(["all", "all", "all", "all", "all", "all", "all"])
    sheet.append(
        [
            "Effect.Burn",
            "Burn",
            "화상",
            "State.Debuff.Burn",
            "/Game/UI/T_Burn.T_Burn",
            3,
            True,
        ]
    )
    source = tmp_path / "types.xlsx"
    workbook.save(source)

    assert _build(source, tmp_path) == 0
    first = output_bytes(tmp_path)
    header = (tmp_path / "cpp" / "DtTypesRow.h").read_text(encoding="utf-8")
    assert "FName Id;" in header
    assert "FString Label;" in header
    assert "FText DisplayName;" in header
    assert "FGameplayTag StateTag;" in header
    assert "FSoftObjectPath Icon;" in header
    assert '#include "Internationalization/Text.h"' in header
    assert '#include "GameplayTagContainer.h"' in header
    assert '#include "UObject/SoftObjectPath.h"' in header

    client = json.loads((tmp_path / "client" / "Types.json").read_text(encoding="utf-8"))
    server = json.loads((tmp_path / "server" / "Types.json").read_text(encoding="utf-8"))
    expected = {
        "Id": "Effect.Burn",
        "Label": "Burn",
        "DisplayName": "화상",
        "StateTag": "State.Debuff.Burn",
        "Icon": "/Game/UI/T_Burn.T_Burn",
        "Count": 3,
        "Enabled": True,
    }
    assert client["rows"] == [expected]
    assert server["rows"] == [expected]

    assert _build(source, tmp_path) == 0
    assert output_bytes(tmp_path) == first


def test_type_specific_includes_are_omitted_when_unused(tmp_path: Path) -> None:
    workbook = Workbook()
    sheet = workbook.active
    sheet.title = "Plain"
    sheet.append(["Id", "Label"])
    sheet.append(["ID<int32>", "string"])
    sheet.append(["all", "all"])
    sheet.append([1, "값"])
    source = tmp_path / "plain.xlsx"
    workbook.save(source)

    assert _build(source, tmp_path) == 0
    header = (tmp_path / "cpp" / "DtPlainRow.h").read_text(encoding="utf-8")
    assert "Internationalization/Text.h" not in header
    assert "GameplayTagContainer.h" not in header
    assert "UObject/SoftObjectPath.h" not in header


@pytest.mark.parametrize("legacy_type", ["FName", "FString"])
def test_legacy_types_are_rejected_with_migration_message(
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
    legacy_type: str,
) -> None:
    workbook = Workbook()
    sheet = workbook.active
    sheet.title = "Legacy"
    sheet.append(["Id", "OldValue"])
    sheet.append(["ID<int32>", legacy_type])
    sheet.append(["all", "all"])
    sheet.append([1, "값"])
    source = tmp_path / f"legacy-{legacy_type}.xlsx"
    workbook.save(source)

    assert main(["check", "--input", str(source)]) == 1
    stderr = capsys.readouterr().err
    assert "[Legacy.schema.xlsx]Legacy!B3" in stderr
    assert "이제 name/string을 쓰세요" in stderr


@pytest.mark.parametrize("value_type", ["string", "tag", "path", "float", "double", "bool"])
@pytest.mark.parametrize("role", ["ID", "SubKey"])
def test_unsupported_key_types_are_rejected_without_outputs(
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
    value_type: str,
    role: str,
) -> None:
    workbook = Workbook()
    sheet = workbook.active
    sheet.title = "InvalidKey"
    sheet.append(["Id", "Value"])
    sheet.append(
        [f"ID<{value_type}>", "string"]
        if role == "ID"
        else ["ID<int32>", f"SubKey<{value_type}>"]
    )
    sheet.append(["all", "all"])
    sheet.append(["Key" if role == "ID" else 1, "Value"])
    source = tmp_path / f"invalid-{role.lower()}-{value_type}.xlsx"
    workbook.save(source)

    assert _build(source, tmp_path / "output") == 1
    stderr = capsys.readouterr().err
    expected_cell = "B2" if role == "ID" else "B3"
    assert f"[InvalidKey.schema.xlsx]InvalidKey!{expected_cell}" in stderr
    assert f"{value_type} 자료형은 기본키나 서브키" in stderr
    assert "int32, int64, name, 열거형(E*)" in stderr
    assert not (tmp_path / "output" / "cpp").exists()
    assert not (tmp_path / "output" / "client").exists()
    assert not (tmp_path / "output" / "server").exists()


def test_supported_key_types_remain_available(tmp_path: Path) -> None:
    workbook = Workbook()
    add_enum(workbook)
    sheet = workbook.create_sheet("SupportedKeys")
    sheet.append(["Id", "Numeric", "Name", "Element"])
    sheet.append(
        ["ID<int64>", "SubKey<int32>", "SubKey<name>", "SubKey<EElement>"]
    )
    sheet.append(["all", "all", "all", "all"])
    sheet.append([9_000_000_001, 7, "Burn", "Fire"])
    source = tmp_path / "supported-keys.xlsx"
    workbook.save(source)

    assert _build(source, tmp_path / "output") == 0


@pytest.mark.parametrize("key_type", ["ID<text>", "SubKey<text>"])
def test_text_cannot_be_used_as_key(
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
    key_type: str,
) -> None:
    workbook = Workbook()
    sheet = workbook.active
    sheet.title = "TextKey"
    sheet.append(["Id", "Localized"])
    sheet.append(["ID<int32>", key_type])
    sheet.append(["all", "all"])
    sheet.append([1, "지역화 값"])
    source = tmp_path / "text-key.xlsx"
    workbook.save(source)

    assert main(["check", "--input", str(source)]) == 1
    stderr = capsys.readouterr().err
    assert "[TextKey.schema.xlsx]TextKey!B3" in stderr
    assert "text 자료형은 기본키나 서브키" in stderr
    assert "int32, int64, name, 열거형(E*)" in stderr


def test_typed_path_syntax_is_rejected(tmp_path: Path, capsys: pytest.CaptureFixture[str]) -> None:
    workbook = Workbook()
    sheet = workbook.active
    sheet.title = "TypedPath"
    sheet.append(["Id", "Icon"])
    sheet.append(["ID<int32>", "path<UTexture2D>"])
    sheet.append(["all", "all"])
    sheet.append([1, "/Game/UI/T_Icon.T_Icon"])
    source = tmp_path / "typed-path.xlsx"
    workbook.save(source)

    assert main(["check", "--input", str(source)]) == 1
    stderr = capsys.readouterr().err
    assert "[TypedPath.schema.xlsx]TypedPath!B3" in stderr
    assert "알 수 없는 자료형" in stderr


def test_manifest_has_no_stamp_by_default(tmp_path: Path) -> None:
    source = tmp_path / "Tables.xlsx"
    save_valid(source)
    assert _build(source, tmp_path) == 0
    manifest = json.loads((tmp_path / "client" / "manifest.json").read_text(encoding="utf-8"))
    assert "generated_at" not in manifest
    assert "generated_at_utc" not in manifest
    assert manifest["source_files"] == ["Tables.xlsx"]


def test_sub_keys_can_be_empty(tmp_path: Path) -> None:
    workbook = Workbook()
    sheet = workbook.active
    sheet.title = "Items"
    sheet.append(["Id", "Name"])
    sheet.append(["ID<name>", "string"])
    sheet.append(["all", "all"])
    sheet.append(["Sword", "검"])
    source = tmp_path / "input.xlsx"
    workbook.save(source)
    assert _build(source, tmp_path) == 0
    payload = json.loads((tmp_path / "client" / "Items.json").read_text(encoding="utf-8"))
    assert payload["sub_keys"] == []


def test_primary_key_scope_must_be_all(
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    workbook = Workbook()
    add_enum(workbook)
    sheet = add_table(workbook)
    sheet["A3"] = "client"
    source = tmp_path / "bad-primary-scope.xlsx"
    workbook.save(source)

    assert main(["check", "--input", str(source)]) == 1
    stderr = capsys.readouterr().err
    assert "[Effects.schema.xlsx]Effects!C2" in stderr
    assert "기본키 범위는 all" in stderr


def test_check_does_not_write_files(tmp_path: Path) -> None:
    source = tmp_path / "Tables.xlsx"
    save_valid(source)
    before = output_bytes(tmp_path)
    assert main(["check", "--input", str(source)]) == 0
    assert output_bytes(tmp_path) == before


@pytest.mark.parametrize(
    ("mutate", "location", "message"),
    [
        (lambda sheet: sheet.__setitem__("A2", "int32"), "[Effects.schema.xlsx]Effects!B2", "기본키"),
        (lambda sheet: sheet.__setitem__("B2", "ID<name>"), "[Effects.schema.xlsx]Effects!B2", "기본키"),
        (lambda sheet: sheet.__setitem__("A5", 1001), "Effects!A5", "중복"),
        (lambda sheet: sheet.__setitem__("A5", None), "Effects!A5", "비어"),
        (lambda sheet: sheet.__setitem__("C2", "EMissing"), "[Effects.schema.xlsx]Effects!B4", "정의되지 않은"),
        (lambda sheet: sheet.__setitem__("D4", "숫자 아님"), "Effects!D4", "변환"),
        (lambda sheet: sheet.__setitem__("B1", "Id"), "[Effects.schema.xlsx]Effects!A3", "중복"),
    ],
)
def test_validation_errors(
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
    mutate: object,
    location: str,
    message: str,
) -> None:
    workbook = Workbook()
    add_enum(workbook)
    sheet = add_table(workbook)
    mutate(sheet)
    source = tmp_path / "bad.xlsx"
    workbook.save(source)
    assert main(["check", "--input", str(source)]) == 1
    stderr = capsys.readouterr().err
    assert location in stderr
    assert message in stderr


@pytest.mark.parametrize(
    ("mutate", "location", "message"),
    [
        (lambda sheet: sheet.__setitem__("G1", "Reward[2]"), "[Effects.schema.xlsx]Effects!A8", "연속"),
        (lambda sheet: sheet.__setitem__("G1", "Reward[0]"), "[Effects.schema.xlsx]Effects!A9", "중복"),
        (lambda sheet: sheet.__setitem__("G2", "float"), "[Effects.schema.xlsx]Effects!B8", "자료형"),
        (lambda sheet: sheet.__setitem__("G3", "client"), "[Effects.schema.xlsx]Effects!C8", "범위"),
    ],
)
def test_array_validation_errors(
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
    mutate: object,
    location: str,
    message: str,
) -> None:
    workbook = Workbook()
    add_enum(workbook)
    sheet = add_table(workbook)
    mutate(sheet)
    source = tmp_path / "bad-array.xlsx"
    workbook.save(source)
    assert main(["check", "--input", str(source)]) == 1
    stderr = capsys.readouterr().err
    assert location in stderr
    assert message in stderr


def _build(source: Path, root: Path) -> int:
    return main(
        [
            "build",
            "--input",
            str(source),
            "--out-cpp",
            str(root / "cpp"),
            "--out-client",
            str(root / "client"),
            "--out-server",
            str(root / "server"),
        ]
    )


def add_unsorted_table(workbook: Workbook) -> None:
    """기본키를 일부러 내림차순으로 넣어 정렬이 실제로 동작하는지 본다.

    Element 열은 열거자 이름 순서(Fire < Water)와 값 순서(Fire=0, Water=1)가
    같지 않도록 Zeta를 끼워 넣는다.
    """
    sheet = workbook.create_sheet("Effects")
    sheet.append(["Id", "Name", "Element", "ServerOnly"])
    sheet.append(["ID<int32>", "SubKey<name>", "SubKey<EElement>", "int32"])
    sheet.append(["all", "all", "all", "server"])
    sheet.append([1003, "Curse", "Water", 3])
    sheet.append([1001, "Burn", "Zeta", 1])
    sheet.append([1002, "Freeze", "Water", 2])


def save_unsorted(path: Path) -> None:
    workbook = Workbook()
    sheet = workbook.active
    sheet.title = "<enum>Element"
    sheet.append(["Id", "Value", "Comment"])
    sheet.append(["ID<name>", "int32", "string"])
    sheet.append(["all", "all", "#"])
    sheet.append(["Zeta", 0, "이름은 뒤지만 값이 앞"])
    sheet.append(["Water", 1, "물"])
    add_unsorted_table(workbook)
    workbook.save(path)


def test_asset_class_header_is_generated(tmp_path: Path) -> None:
    source = tmp_path / "Tables.xlsx"
    save_valid(source)
    assert _build(source, tmp_path) == 0
    header = (tmp_path / "cpp" / "DtEffectsTable.h").read_text(encoding="utf-8")
    assert "class UDtEffectsTable : public UPrimaryDataAsset" in header
    assert '#include "Engine/DataAsset.h"' in header
    assert '#include "DtEffectsRow.h"' in header
    assert '#include "EDtElement.h"' in header
    assert "TArray<FDtEffectsRow> Rows;" in header
    assert "TArray<int32> PrimaryKeys;" in header
    assert "TArray<FName> Name_Keys;" in header
    assert "TArray<int32> Name_Offsets;" in header
    assert "TArray<int32> Name_Indices;" in header
    assert "TArray<EDtElement> Element_Keys;" in header


def test_asset_class_without_sub_keys_has_no_index_arrays(tmp_path: Path) -> None:
    workbook = Workbook()
    sheet = workbook.active
    sheet.title = "Items"
    sheet.append(["Id", "Name"])
    sheet.append(["ID<name>", "string"])
    sheet.append(["all", "all"])
    sheet.append(["Sword", "검"])
    source = tmp_path / "items.xlsx"
    workbook.save(source)
    assert _build(source, tmp_path) == 0
    header = (tmp_path / "cpp" / "DtItemsTable.h").read_text(encoding="utf-8")
    assert "TArray<FName> PrimaryKeys;" in header
    assert "_Keys;" not in header
    assert "_Offsets;" not in header
    assert "_Indices;" not in header


def test_asset_base_option_uses_given_class_and_header(tmp_path: Path) -> None:
    source = tmp_path / "Tables.xlsx"
    save_valid(source)
    code = main(
        [
            "build",
            "--input",
            str(source),
            "--out-cpp",
            str(tmp_path / "cpp"),
            "--out-client",
            str(tmp_path / "client"),
            "--out-server",
            str(tmp_path / "server"),
            "--asset-base",
            "UDtTableAsset",
            "--asset-base-header",
            "TableData/DtTableAsset.h",
        ]
    )
    assert code == 0
    header = (tmp_path / "cpp" / "DtEffectsTable.h").read_text(encoding="utf-8")
    assert "class UDtEffectsTable : public UDtTableAsset" in header
    assert '#include "TableData/DtTableAsset.h"' in header
    assert '#include "Engine/DataAsset.h"' not in header


def test_asset_base_without_header_is_usage_error(
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    source = tmp_path / "Tables.xlsx"
    save_valid(source)
    with pytest.raises(SystemExit) as excinfo:
        main(
            [
                "build",
                "--input",
                str(source),
                "--out-cpp",
                str(tmp_path / "cpp"),
                "--out-client",
                str(tmp_path / "client"),
                "--out-server",
                str(tmp_path / "server"),
                "--asset-base",
                "UDtTableAsset",
            ]
        )
    assert excinfo.value.code == 2
    assert "--asset-base-header" in capsys.readouterr().err


def test_client_rows_are_sorted_by_primary_key(tmp_path: Path) -> None:
    source = tmp_path / "unsorted.xlsx"
    save_unsorted(source)
    assert _build(source, tmp_path) == 0
    client = json.loads((tmp_path / "client" / "Effects.json").read_text(encoding="utf-8"))
    server = json.loads((tmp_path / "server" / "Effects.json").read_text(encoding="utf-8"))
    assert [row["Id"] for row in client["rows"]] == [1001, 1002, 1003]
    assert client["primary_keys"] == [1001, 1002, 1003]
    # 서버 JSON도 같은 행 순서를 쓴다(인덱스만 없다).
    assert [row["Id"] for row in server["rows"]] == [1001, 1002, 1003]


def test_client_csr_index_is_valid_and_enum_keys_sort_by_value(tmp_path: Path) -> None:
    source = tmp_path / "unsorted.xlsx"
    save_unsorted(source)
    assert _build(source, tmp_path) == 0
    client = json.loads((tmp_path / "client" / "Effects.json").read_text(encoding="utf-8"))
    rows = client["rows"]
    by_name = {item["name"]: item for item in client["sub_keys"]}

    for entry in client["sub_keys"]:
        keys, offsets, indices = entry["keys"], entry["offsets"], entry["indices"]
        assert len(offsets) == len(keys) + 1
        assert offsets[-1] == len(indices)
        assert offsets == sorted(offsets)
        for position, key in enumerate(keys):
            bucket = indices[offsets[position] : offsets[position + 1]]
            assert bucket, f"빈 버킷: {key}"
            for index in bucket:
                assert rows[index][entry["field"]] == key

    # 열거형 서브키는 이름(Water < Zeta)이 아니라 값(Zeta=0, Water=1) 순서를 따른다.
    assert by_name["Element"]["keys"] == ["Zeta", "Water"]
    assert by_name["Name"]["keys"] == ["Burn", "Curse", "Freeze"]


def test_server_json_has_no_index(tmp_path: Path) -> None:
    source = tmp_path / "unsorted.xlsx"
    save_unsorted(source)
    assert _build(source, tmp_path) == 0
    server = json.loads((tmp_path / "server" / "Effects.json").read_text(encoding="utf-8"))
    assert "primary_keys" not in server
    for entry in server["sub_keys"]:
        assert set(entry) == {"name", "field"}
