# DrTableSystem

**DesignToRuntime Table System** — carries the data designers write in spreadsheets all the way to the game runtime.

[한국어](README.ko.md)

- **Schemas apart from data.** Tables and enums are defined in schema workbooks (`.schema.xlsx`, `.enum.xlsx`) owned by programmers. Designers edit data workbooks. Generated code depends on the schemas only, so data edits never change code.
- **Unreal C++**: `USTRUCT` rows, `UENUM` enums, a DataAsset class per table, typed lookups (`Find`, `FindBy<SubKey>`, `Get<Ref>`).
- **Client and server JSON** from the same data, split by field scope.
- **Unreal plugin**: bakes client JSON into DataAssets with prebuilt key indices, loads them without copying, and rejects stale assets.
- **References between tables** (`Ref<Items>`, `Ref<DropTable.GroupId>`), checked by `drtable check` and drawn as a Mermaid graph.
- **GUI** (`drtable-gui`): build and check with clickable errors, browse tables, see the reference graph, create data workbooks.
- Split a table over sheets and files, deterministic output, messages in English or Korean.

```
Schema/*.schema.xlsx       ─┐                 ┌─▶ C++ headers ─▶ compile
Enums/*.enum.xlsx          ─┼▶ drtable build ─┼─▶ client JSON ─▶ DrTableBake ─▶ DataAssets ─▶ runtime lookups
Data workbooks (*.xlsx)    ─┘                 └─▶ server JSON ─▶ your server
```

## Quick start

```sh
drtable build --input Design/Tables --schema Design/Tables/Schema --ue-plugin --prefix Gm \
  --out-cpp Source/MyGame/TableData/Generated --out-client Intermediate/DrTable/client --out-server Build/ServerData
drtable check --client Intermediate/DrTable/client --server Build/ServerData
drtable new --table Items --out Design/Tables/Items.xlsx --schema Design/Tables/Schema   # new data workbook
```

A table schema (`Design/Tables/Schema/Items.schema.xlsx`, one field per row from row 2):

| Field | Type | Scope | Comment |
|---|---|---|---|
| Id | `ID<int32>` | all | |
| Kind | `SubKey<EItemType>` | all | |
| Price | `int32=0` | server | sell price |

A data workbook has the field names in row 1 (any column order) and data from row 4; the sheet name is the table name.

![drtable-gui](docs/images/en/build.png)

## Documentation

- Manual: [English](docs/en/manual.md) · [한국어](docs/ko/manual.md)
- Unreal plugin: `unreal/DrTableSystem` (Unreal Engine 5.8)

`drtable` is a single executable with nothing to install (download it from Releases). To build it: `cd rust && cargo build --release`. `src/drtable` is a Python reference implementation; the tests check that both produce identical output.
