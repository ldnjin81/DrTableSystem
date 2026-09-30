# DrTableSystem

**DesignToRuntime Table System** — carries the data designers write in spreadsheets all the way to the game runtime.

[한국어](README.ko.md)

- **Schemas apart from data.** Tables and enums are defined in schema files (`.schema.xlsx` or `.schema.yaml`) owned by programmers. Designers edit data workbooks. Generated code depends on the schemas only, so data edits never change code.
- **Unreal C++**: `USTRUCT` rows, `UENUM` enums, a DataAsset class per table, typed lookups (`Find`, `FindBy<SubKey>`, `Get<Ref>`).
- **Client and server JSON** from the same data, split by field scope.
- **Unreal plugin**: bakes client JSON into DataAssets with prebuilt key indices, loads them without copying, and rejects stale assets.
- **References between tables** (`Ref<Items>`, `Ref<DropTable.GroupId>`), checked by `drtable check` and drawn as a Mermaid graph.
- Split a table over sheets and files, deterministic output, messages in English or Korean.

```
Schemas/*.schema.xlsx|yaml ─┐                 ┌─▶ C++ headers ─▶ compile
Schemas/Enums/*.enum.*     ─┼▶ drtable build ─┼─▶ client JSON ─▶ DrTableBake ─▶ DataAssets ─▶ runtime lookups
Data workbooks (*.xlsx)    ─┘                 └─▶ server JSON ─▶ your server
```

## Quick start

```sh
uv sync
uv run drtable build --input Design/Tables --schema Design/Tables/Schemas --ue-plugin --prefix Gm \
  --out-cpp Source/MyGame/TableData/Generated --out-client Intermediate/DrTable/client --out-server Build/ServerData
uv run drtable check --client Intermediate/DrTable/client --server Build/ServerData
uv run drtable headers --input Design/Tables --schema Design/Tables/Schemas   # show the schema in rows 2-3
```

A table schema (`Design/Tables/Schemas/Items.schema.yaml`):

```yaml
table: Items
fields:
  - {name: Id,     type: ID<int32>,     scope: all}
  - {name: Kind,   type: SubKey<EItemType>, scope: all}
  - {name: Price,  type: int32=0,       scope: server}
```

A data workbook has the field names in row 1 (any column order) and data from row 4; the sheet name is the table name.

## Documentation

- Manual: [English](docs/en/manual.md) · [한국어](docs/ko/manual.md)
- Unreal plugin: `unreal/DrTableSystem` (Unreal Engine 5.8)

Requirements: Python 3.12+, `openpyxl`, `pyyaml`.
