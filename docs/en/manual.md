# ue-tablegen manual

ue-tablegen turns spreadsheet tables into:

- **Unreal C++**: `USTRUCT` row types, `UENUM` enums, one DataAsset class per table, and optionally typed lookup functions and a registration header.
- **Client JSON**: the rows a game client needs, plus prebuilt key indices, ready to be baked into DataAssets.
- **Server JSON**: the rows a server needs (server-only fields included, client-only fields excluded).

The **TableGen Unreal plugin** bakes the client JSON into DataAssets, loads them at runtime without copying or building indices, and detects stale assets.

The spreadsheet is the single source of truth for both the schema and the data.

```
GameData.xlsx ─▶ tablegen build ─┬─▶ C++ headers ──────────▶ compile
                                 ├─▶ client JSON ─▶ TableGenBake ─▶ DA_*.uasset ─▶ runtime lookups
                                 └─▶ server JSON ─▶ your server
                 tablegen check  ◀─ client/server JSON   (reference integrity, CI)
                 tablegen graph  ─▶ references.md       (Mermaid diagram)
```

Contents

1. [Install](#1-install)
2. [Spreadsheet format](#2-spreadsheet-format)
3. [Types](#3-types)
4. [Keys, sub keys and defaults](#4-keys-sub-keys-and-defaults)
5. [Arrays](#5-arrays)
6. [Enums](#6-enums)
7. [References between tables](#7-references-between-tables)
8. [Outputs](#8-outputs)
9. [Command line](#9-command-line)
10. [Unreal plugin](#10-unreal-plugin)
11. [Change detection: schema and content hashes](#11-change-detection-schema-and-content-hashes)
12. [Rules that keep lookups correct](#12-rules-that-keep-lookups-correct)
13. [CI](#13-ci)
14. [Troubleshooting](#14-troubleshooting)

---

## 1. Install

Requirements: Python 3.12+, [uv](https://docs.astral.sh/uv/) (recommended). The only runtime dependency is `openpyxl`.

```sh
git clone <this repository> ue-tablegen
cd ue-tablegen
uv sync
uv run tablegen --version
```

For the Unreal side, copy `unreal/TableGen` into your project's `Plugins/` folder (see [section 10](#10-unreal-plugin)). The plugin was developed and tested with Unreal Engine 5.8.

Messages are printed in English by default. Use `--lang ko` or `TABLEGEN_LANG=ko` for Korean. Generated files are always English.

## 2. Spreadsheet format

### Sheets

| Sheet name | Meaning |
|---|---|
| starts with `#` | Notes. Ignored. |
| `<enum>Name` | An enum definition (section 6). |
| anything else | A table. The sheet name is the table name. |

Table and enum names become C++ identifiers: they must start with a letter and contain only letters, digits and `_`. You can pass a single `.xlsx` or a folder; every `.xlsx` in the folder is read (files starting with `~$` are ignored).

### Three header rows

| Row | Content |
|---|---|
| 1 | Scope: `all` both, `client` client only, `server` server only, `#` comment (excluded everywhere). Case-insensitive. |
| 2 | Type, optionally with a key role or a default: `int32`, `ID<int32>`, `SubKey<name>`, `float=1.0` |
| 3 | Field name (a C++ identifier), right above the data so the sheet reads like a table |
| 4+ | Data |

An empty field name (row 3) ends the header: columns to the right are not read. Rows whose data cells are all empty are skipped.

Example sheet `Effects`:

| | A | B | C | D |
|---|---|---|---|---|
| **1** | `all` | `all` | `all` | `client` |
| **2** | `ID<int32>` | `SubKey<name>` | `SubKey<EElement>` | `float` |
| **3** | `Id` | `Name` | `Element` | `Damage` |
| **4** | `1001` | `Burn` | `Fire` | `12.5` |
| **5** | `1002` | `Freeze` | `Water` | `0` |

Every error message starts with the sheet and cell, for example `Effects!C7: duplicate primary key '1001'`.

## 3. Types

| Row 2 | C++ | Client JSON | Server JSON |
|---|---|---|---|
| `int32`, `int64` | `int32`, `int64` | number | number |
| `float`, `double` | `float`, `double` | number | number |
| `bool` | `bool` | true/false | true/false |
| `name` | `FName` | string | string |
| `string` | `FString` | string | string |
| `text` | `FText` | string | string |
| `tag` | `FGameplayTag` | string | string |
| `path` | `FSoftObjectPath` | string | string |
| `E<Enum>` | `E<Prefix><Enum>` | enumerator name | enumerator name |
| `Ref<Table>` / `Ref<Table.SubKey>` | the target key's type | same as target | same as target |

- The server is not assumed to be Unreal, so `name`, `string`, `text`, `tag` and `path` are all plain strings in server JSON.
- `path` is always an untyped `FSoftObjectPath`. Convert it at runtime (`TSoftObjectPtr<T>(Path)`) if you need a typed pointer.
- `bool` accepts TRUE/FALSE, 1/0 and the strings `true`/`false`.
- Nested structs are not supported. Use arrays (section 5) or another table with a reference (section 7).

## 4. Keys, sub keys and defaults

- **Primary key**: `ID<type>`. Exactly one per table, scope must be `all`. Values must be unique and not empty.
- **Sub key**: `SubKey<type>`. Any number per table. Each sub key gets a prebuilt index so that "all rows whose `Element` is `Fire`" is a binary search, not a scan. Values need not be unique.
- Key types are limited to `int32`, `int64`, `name` and enums. Floats compare unreliably, `bool` is not a useful key, and `string`/`text`/`tag`/`path` do not have a comparison that matches the generator's sort order.
- **Defaults**: `float=1.0`, `bool=true`, `string=None`. An empty cell uses the declared default, otherwise the type default (0, false, empty, the first enumerator). Keys cannot have defaults.

## 5. Arrays

Columns named `Field[0]`, `Field[1]`, … form one fixed-size array field.

| | A | B | C | D |
|---|---|---|---|---|
| **1** | `all` | `all` | `all` | `all` |
| **2** | `ID<int32>` | `int32` | `int32` | `int32` |
| **3** | `Id` | `Reward[0]` | `Reward[1]` | `Reward[2]` |

- C++: a C-style array, `int32 Reward[3] = {};`. No heap allocation; the values live inside the row.
- JSON: a real array, `"Reward": [10, 20, 30]`.
- Indices start at 0 without gaps. All elements share one type and scope. Arrays cannot be keys.
- Unreal cannot expose C-style arrays to Blueprint, so array properties are `EditAnywhere` only.
- Each column may declare its own default (`int32=10`, `int32=20`).

## 6. Enums

An enum sheet uses the same three-row format. Its primary key column holds the enumerator names.

Sheet `<enum>ItemType`:

| | A | B | C |
|---|---|---|---|
| **1** | `all` | `all` | `#` |
| **2** | `ID<name>` | `int32` | `string` |
| **3** | `Id` | `Value` | `Comment` |
| **4** | `Weapon` | `0` | `Swords, bows` |
| **5** | `Armor` | `1` | `Helmets, plates` |

- `Value` is optional. Missing values count up from 0. Values must fit in uint8 and be unique.
- A `#`-scoped column named `Comment` becomes a code comment.
- **Only `Id` and `Value`** → just the enum (`EDtItemType`).
- **Extra columns** → the enum plus a table `ItemTypeInfo` keyed by the enum, with the extra columns as fields.

Use the enum in a table as `E<Name>` (`EItemType`).

## 7. References between tables

A field of type `Ref<Items>` holds a primary key of the `Items` table. The field's actual type is that key's type, so changing the target key type updates the referencing fields too.

```
Quests:   Id: ID<int32>   RewardItem: Ref<Items>   Next[0]: Ref<Quests>   Next[1]: Ref<Quests>
```

- An **empty cell means "no reference"**: `0` for numeric keys, an empty name for `name` keys. References to enum-keyed tables cannot be empty (an enum has no "none" value).
- Self references and cycles between tables are allowed. Arrays and `SubKey<Ref<...>>` are allowed. `ID<Ref<...>>` and defaults on references are not.

**Sub key references.** `Ref<DropTable.GroupId>` points at the rows whose sub key `GroupId` has that value: one value, many rows.

```
Monsters:   Id: ID<name>        DropGroup: Ref<DropTable.GroupId>
DropTable:  Id: ID<int32>       GroupId: SubKey<int32>      Item: Ref<Items>
```

- The target field must be declared `SubKey<>` (so it has an index). Use `Ref<Table>` for the primary key.
- The reference's scope must not be wider than the target sub key's scope (an `all` field cannot point at a `client`-only sub key: the server output would not have it).
- If the target sub key is itself a reference, the type follows the chain; a chain that loops is reported with the full path.

**Validation.** `build` only checks the schema (the target exists, types resolve). Whether each value actually exists is checked by `tablegen check` on the generated JSON (section 9). Generated C++ carries `meta = (TableRef = "Items")` (and `TableRefKey = "GroupId"` for sub key references) so editor tools can follow references too.

## 8. Outputs

### C++

```
<out-cpp>/EDtElement.h          one per enum
<out-cpp>/DtEffectsRow.h        row struct per table
<out-cpp>/DtEffectsTable.h      DataAsset class per table
<out-cpp>/DtGeneratedTables.h   names, keys, schema and content hash constants
```

With `--runtime-header` (or `--ue-plugin`) also:

```
<out-cpp>/DtEffectsRow.cpp        lookup and reference accessor definitions
<out-cpp>/DtTableRegistration.h   DtGeneratedTables::RegisterAll(Registry)
```

- Row struct `F<Prefix><Table>Row`, asset class `U<Prefix><Table>Table`, enum `E<Prefix><Enum>`. `--prefix` defaults to `Dt`; choose your project's prefix.
- Only client fields (`all`, `client`) are generated.
- The asset class holds `Rows` (sorted by primary key), `PrimaryKeys` (same order), and for each sub key `<Name>_Keys`, `<Name>_Offsets`, `<Name>_Indices` (a CSR index computed by the generator).

**Row functions** (with `--runtime-header`), plain C++ members, not Blueprint-exposed:

| Function | Generated for | Returns |
|---|---|---|
| `static const FRow* Find(Key)` | every table | the row or nullptr |
| `static TArray<const FRow*> FindBy<SubKey>(Key)` | each sub key | matching rows |
| `static TConstArrayView<FRow> GetAll()` | every table | all rows |
| `const FTargetRow* Get<Field>() const` | `Ref<Target>` fields | the referenced row or nullptr |
| `TArray<const FTargetRow*> Get<Field>() const` | `Ref<Target.SubKey>` fields | the referenced rows |
| `Get<Field>(int32 Index) const` | reference arrays | as above for one element |

An empty reference returns nullptr / an empty array without a lookup. A generated name that clashes with a field (for example a field called `Find`) is a build error.

The generated functions call three templates that the runtime header must provide. The Unreal plugin's `TableGenRuntime.h` implements them; you can implement them yourself for another host:

```cpp
namespace TableGenRuntime {
  template <typename TRow, typename TKey> const TRow* FindByKey(const TKey& Key);
  template <typename TRow, typename TKey> TArray<const TRow*> FindAllBySubKey(FName SubKeyName, const TKey& Key);
  template <typename TRow> TConstArrayView<TRow> GetAll();
}
```

`RegisterAll(Registry)` needs a registry with `Register<Row, Asset>(FName Id, TArray<Row> Asset::*Rows, TArray<Key> Asset::*Keys)` returning an object that chains `WithSchemaHash(const TCHAR*)`, `WithContentHash(const TCHAR*)` and `WithSubKey(FName, Keys, Offsets, Indices)`.

### JSON

Client `Effects.json`:

```json
{
  "table": "Effects",
  "schema_hash": "sha256:…",
  "content_hash": "sha256:…",
  "primary_key": "Id",
  "primary_keys": [1001, 1002],
  "sub_keys": [{"name": "Element", "field": "Element",
                "keys": ["Fire", "Water"], "offsets": [0, 1, 2], "indices": [0, 1]}],
  "rows": [{"Id": 1001, "Name": "Burn", "Element": "Fire"}, …]
}
```

Server JSON has the same shape without the indices, and with the server fields. `manifest.json` in each folder lists the tables (rows, schema and content hash), the enums, the references, and the naming rules (`cpp_prefix`, `asset_name`) the bake tool uses.

Output is **deterministic**: the same input produces the same bytes (LF line endings, stable key order, no timestamps unless you pass `--stamp`). Output folders are cleared before writing so deleted tables do not linger.

## 9. Command line

```sh
tablegen build --input <xlsx|folder> --out-cpp <dir> --out-client <dir> --out-server <dir>
               [--prefix Dt] [--ue-plugin] [--asset-base <Class> --asset-base-header <Header.h>]
               [--runtime-header <Header.h>] [--asset-name DA_{table}] [--stamp <ISO8601>]
tablegen graph --input <xlsx|folder> --out references.md
tablegen check --client <client json dir> [--server <server json dir>]
tablegen check --input <xlsx|folder>          # validate only, write nothing
tablegen [--lang en|ko] …
```

| Option | Meaning |
|---|---|
| `--prefix` | C++ type prefix (`Dt` → `FDtEffectsRow`). |
| `--ue-plugin` | Use the TableGen plugin: asset base `UTableGenAssetBase`, runtime header `TableGenRuntime.h`. Recommended with the plugin. |
| `--asset-base`, `--asset-base-header` | Base class of the asset classes and the header that declares it. Changing the base without the header is an error (the code would not compile). |
| `--runtime-header` | Enables row functions, `.cpp` files and the registration header. |
| `--asset-name` | Asset name pattern for registration and baking. Must contain `{table}`. |
| `--stamp` | Adds `generated_at` to the manifest (for CI traceability; breaks byte-for-byte determinism on purpose). |

`graph` writes a Mermaid `flowchart` (renders on GitHub): one node per table with its key type, one arrow per reference labelled with the field, `[N]` for arrays, `(SubKey)` when the referencing field is a sub key and `→ Key 1:N` for sub key references.

`check --client/--server` reads only generated JSON, so it can run in CI. It prints every broken reference, for example `Quests.Next[2002](0) = 9999 → Quests: not in the table`, skips empty references, and warns when a target has a key equal to the "no reference" value (0 or empty).

Exit codes: `build`/`graph`/`check --input` → 0 ok, 1 validation errors, 2 usage error. `check --client` → 0 ok, 1 broken references, 2 invalid input.

## 10. Unreal plugin

`unreal/TableGen` contains two modules:

- **TableGenRuntime**: `UTableGenAssetBase`, `TTableGenRowTable`, `UTableGenRegistry` (hosted by the engine subsystem `UTableGenSubsystem`), `UTableGenSettings`, the `TableGenRuntime` contract and `TABLEGEN_AUTO_REGISTER`.
- **TableGenEditor**: the `TableGenBake` commandlet and the plugin's automation tests (`TableGen.*`).

### Setup

1. Copy `unreal/TableGen` to `<Project>/Plugins/TableGen` and enable it (`"Plugins": [{"Name": "TableGen", "Enabled": true}]`).
2. Add `"TableGenRuntime"` to your game module's `PublicDependencyModuleNames`.
3. Generate into your module's source tree:
   ```sh
   tablegen build --input Design/GameData.xlsx --prefix Gm --ue-plugin \
     --out-cpp Source/MyGame/TableData/Generated \
     --out-client Intermediate/TableGen/client --out-server Build/ServerData
   tablegen check --client Intermediate/TableGen/client --server Build/ServerData
   ```
4. Register the generated tables once, in any `.cpp` of that module:
   ```cpp
   #include "TableGenRegistry.h"
   #include "TableData/Generated/GmTableRegistration.h"

   TABLEGEN_AUTO_REGISTER(GmGeneratedTables::RegisterAll<UTableGenRegistry>);
   ```
5. Build the editor, then bake:
   ```sh
   UnrealEditor-Cmd MyGame.uproject -run=TableGenBake -Input=Intermediate/TableGen/client -Out=/Game/Data
   ```
6. Use the rows:
   ```cpp
   if (const FGmItemsRow* Sword = FGmItemsRow::Find(1001))
   {
       const FGmQuestsRow* Quest = Sword->GetQuest();          // Ref<Quests>
   }
   for (const FGmItemsRow* Weapon : FGmItemsRow::FindByKind(EGmItemType::Weapon)) { … }
   ```

### Loading

On first use, the registry loads every registered table from `<AssetRoot>/<AssetName>.<AssetName>` (Project Settings → Plugins → TableGen; default `/Game/Data`). `ExtraAssets` adds assets or overrides the path of a table with the same asset name. Set `bLoadOnFirstUse` to false to call `UTableGenRegistry::Get()->LoadAllTables()` yourself.

Rows are read in place from the asset; nothing is copied and no index is built at runtime. **Pointers and views returned by lookups stay valid until the tables are reloaded** (`TableGen.Reload`, or re-baking in the editor). Look rows up again instead of keeping pointers across a reload.

Console commands: `TableGen.Status` (tables and loaded row counts), `TableGen.Reload`.

### Baking

```
-run=TableGenBake -Input=<client json dir> [-Out=/Game/Data] [-Force] [-Verify]
```

- Class and asset names come from the manifest, so they always match the generated code.
- Only tables whose asset is missing or whose hashes differ are saved. Re-saving identical data would still change the binary package (GUIDs), so unchanged tables are skipped. `-Force` re-saves all.
- `-Verify` saves nothing and fails when any asset is missing or out of date.
- A table whose class is not compiled into the editor is an error: build after generating, then bake.

### Using your own runtime

The generator does not depend on the plugin. Without `--ue-plugin` you get plain `UPrimaryDataAsset` classes and no lookup functions. With `--runtime-header MyRuntime.h` and `--asset-base`, you can host tables in your own system as long as it implements the contract in section 8.

## 11. Change detection: schema and content hashes

Every table has two hashes, written to the JSON, the manifest, the generated C++ and (by the bake) the asset:

| Hash | Covers | Differs when | At load time |
|---|---|---|---|
| `schema_hash` | fields, types, key roles, scopes, declared defaults, and the enums the table uses | the layout changed | **error**, table not loaded |
| `content_hash` | the rows and indices in that output | any value changed | **warning**, table loaded |

So both "I changed the spreadsheet structure but did not rebuild/re-bake" and "I changed a value but did not re-bake" are reported instead of silently using stale data. `TableGenBake -Verify` catches both before shipping.

## 12. Rules that keep lookups correct

Lookups binary-search arrays sorted by the generator, so the generator's order and the runtime comparison must agree exactly:

- Numbers sort numerically.
- **Enums sort by value, not name.**
- **Names sort by Unicode code point, case-sensitively** (not `FName::LexicalLess`, which ignores case and compares numeric suffixes as numbers).

Consequences enforced by the tools:

- `name` keys and sub key values that differ **only by case** are an error (`Sword` vs `sword`): Unreal's `FName` treats them as the same name.
- Changing an enum's values changes the schema hash of every table that uses it, so stale indices are rejected.
- Key type must match exactly at runtime: a table keyed by `int32` is not found with an `int64` key.

## 13. CI

A typical pipeline:

```sh
tablegen build … --ue-plugin
tablegen check --client … --server …          # broken references fail the build
UnrealEditor-Cmd … -run=TableGenBake -Input=… -Verify   # stale or missing assets fail the build
```

`.github/workflows/ci.yml` runs the Python tests and lint on every push.

## 14. Troubleshooting

| Message | Cause and fix |
|---|---|
| `Schema mismatch, re-bake required` | The asset was baked from an older layout. Re-generate, build, bake. |
| `Data changed since the asset was baked` | The spreadsheet values changed. Run the bake (only changed tables are saved). |
| `Class U…Table is not compiled into the editor` | Build the editor after `tablegen build`, then bake. |
| `Table asset not found` | The table is registered but not baked, or `AssetRoot` / `--asset-name` differ between baking and loading. |
| `Row type is registered for more than one table` | Two registrations share a row struct; look rows up by table id (`FindRowByKey<TRow>(TableId, Key)`). |
| `--asset-base requires --asset-base-header` | Pass the header that declares the base class, or use `--ue-plugin`. |
| `generated function 'Find' clashes with a field` | Rename the field; `Find`, `GetAll`, `FindBy<SubKey>` and `Get<Field>` are generated names. |
| `name key 'x' differs from 'X' … only by case` | Make the spellings identical or use different names. |
