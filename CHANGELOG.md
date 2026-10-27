# Changelog

## 0.11.0

Tables can be scoped to one side. Schemas whose primary keys are `all` (the only scope allowed so far) build byte-identically.

- The primary key's scope is the table scope: `all` (both sides), `client` (client JSON and C++ only), `server` (server JSON only, no C++ row, asset or registration) or `#` (a design-only table that is not built, like a `#` field). The other side's JSON folder and manifest leave a one-sided table out.
- Field scopes narrow to the table scope (`all` in a `client` table goes to the client). A field for the other side (a `client` field in a `server` table, or the reverse) is warned about at its schema cell and left out, since it could never be emitted; its data column is allowed and ignored.
- A reference may not reach a table missing on its side: the existing sub-key rule now also covers `Ref<Table>` (`reference scope all is wider than the target table scope server`).

## 0.10.0

Packaged builds now contain the table assets, and the bake rejects paths to assets that do not exist. Generated code and baked data are unchanged.

- The editor module adds every table asset and string asset under `AssetRoot`, plus `ExtraAssets`, to the cook (`UE::Cook::FDelegates::ModifyCook`). The table subsystem loads them by path, so nothing referenced them and a packaged game logged `Table asset not found`. Remove any `DirectoriesToAlwaysCook` entry you added for the table folder as a workaround.
- `DrTableBake` checks every `path` value, array elements included, against the asset registry and fails when one points nowhere, naming the cell (`[GameAssets.Rows[0].Path] Asset path does not exist: …`). It checks on every run, including `-Verify` and unchanged tables, and leaves the existing asset untouched on failure. Empty paths are allowed; a Blueprint class path (`…_C`) is accepted when its Blueprint exists.
- A project without string tables no longer logs a missing `DA_DrStrings` package at startup: the string subsystem checks that the manifest exists before loading it.
- `UDrTableSubsystem::AreAllTablesLoaded()` reports whether at least one table is registered and every registered table loaded, for startup checks and smoke tests.

## 0.9.0

Breaking: the table registry is merged into the table subsystem.

- `UDrTableRegistry` is gone. `UDrTableSubsystem` (engine subsystem) now registers, loads and looks up tables itself: `UDrTableSubsystem::Get()`, `SetOverride` / `ClearOverride`, `RegisterAutoTables`, `LoadAllTables`, `Register`, `FindRowByKey`, … keep their signatures. Include `DrTableSubsystem.h` instead of `DrTableRegistry.h`, and register with `DRTABLE_AUTO_REGISTER(MyGeneratedTables::RegisterAll<UDrTableSubsystem>)`.
- Generated `RegisterAll` takes `TTables& Tables` (was `TRegistry& Registry`); the generated lookups are unchanged.
- Tests can still create standalone instances with `NewObject<UDrTableSubsystem>()` and make them active with `SetOverride`.

## 0.8.0

Value rules, Excel validation, build comparison and a translation workflow. Nothing changes in the output of schemas and data that do not use the new Rule column.

- Table schemas take an optional fifth column `Rule` (or `규칙`): number ranges (`0..100`, `>=0`, `<1000`), `required`, text length (`len<=32`) and `regex:…`, several joined with `;`. `build` and `check` test every value, array elements and defaults included, and point at the schema or data cell. Without the column the output is byte-identical to 0.7.0.
- `drtable l10n` adds translation progress reports, focused export/import workbooks, and unused string-key reports. Import reads the current source folder and writes only new string workbooks under a separate output folder.
- `drtable diff` compares two generated client or server JSON folders by table, schema hash, primary key, field, array element, and string language. Text, Markdown, and JSON reports support a table filter and optional change exit code. Optional `--schema` shows fixed-point values as decimals without changing build output.
- `drtable new` adds enum dropdowns and Excel number/text-length validation to new data workbooks. Long enum lists use a hidden sheet; reference keys and comma-separated arrays remain build-validated.

## 0.7.0

Breaking: arrays are declared differently, and generated properties are read-only.

- Arrays are one schema field with an array type: `Reward | int32[]` (any length) or `int32[4]` (a maximum). Each row sets its own length. Data sheets write element columns `Reward[0]`, `Reward[1]`, … (as many as needed; empty cells at the end are not elements, a gap is an error) or one comma-separated cell `Reward | 10, 20, 30`, chosen per sheet. The old per-element schema rows (`Reward[0]`, `Reward[1]`) are an error that shows the new form. Arrays have no default value.
- Client JSON and assets keep each array field's elements in one pool per table (`"arrays": [{"field", "pool"}]`, asset `<Field>_Pool`); rows hold `<Field>_Start` and `<Field>_Num`. Server JSON keeps each row's array. The manifest's references say `"array": true|false` instead of `array_length`.
- Generated C++: `TConstArrayView<T> Get<Field>() const` returns a row's elements (runtime `DrTableRuntime::GetArray`, registry `WithArray`); reference arrays keep `Get<Field>(int32 Index)`. Array members that clash with a field are an error.
- Every generated property is `VisibleAnywhere, BlueprintReadOnly` (was `EditAnywhere`); array data is readable from Blueprints through `<Field>_Start`, `<Field>_Num` and `<Field>_Pool`.
- Type aliases take an array suffix in fields (`ItemID[]`); an alias cannot itself be an array.
- `drtable new` reference formulas show the field's type and scope above element columns too.
- Unreal plugin: `TDrTableRowTable::WithArray` / `GetArray`, `UDrTableRegistry::GetArray`; `DrTableBake` bakes the pools (datetime / duration elements as ticks).

## 0.6.0

- Time types: `datetime` / `datetime<zone>` with IANA zones (`Asia/Seoul`, daylight saving time by date), `UTC`, `GMT`, `KST`, `JST`, `HKT`, `SGT` or `±HH:MM`; ambiguous or seasonal abbreviations (CST, IST, EST) are errors (Excel values read in that zone, stored as Unix milliseconds UTC; a written `Z`/offset wins) and `duration` (time cells, `1:30:00`, `90s`, `1h30m`, `2d`, `500ms`, plain seconds; milliseconds). C++ `FDateTime` / `FTimespan` with `DrTimeZone` metadata; `DrTableBake` converts the milliseconds to ticks.
- Excel date cells keep milliseconds.

## 0.5.0

- Type aliases: `*.using.xlsx` in the schema folder names a type once (`ItemID | int32`, `ItemRef | Ref<Items>`, `Rate | fixed<10000>`, `Level | int32=1`); fields use it as a type, key (`ID<ItemID>`) or with a default. C++ fields keep the underlying type with `meta = (DrType = "...")`, and `<Prefix>Types.h` declares `using` aliases for game code. The GUI shows the alias next to the type.

## 0.4.0

- Fixed-point types `fixed<N>` (int32) and `fixed64<N>` (int64), N a power of ten: decimals or percentages in Excel become exact integer counts of 1/N in JSON, assets and C++ (with a `<Field>Scale` constant and `DrFixedScale` metadata). Values finer than the scale or out of range are errors.
- A declared default fills empty cells as converted once (a fixed-point default used to be scaled twice).

## 0.3.1

- String table data sheets are named after the table (`UIString`); the short form (`UI`) is no longer accepted. `drtable new --table` takes the full name.
- Manual: string tables in the overview and the other chapters; link and example fixes.

## 0.3.0

- String tables: `<Name>String.string.xlsx` lists the languages (code, base mark, scope); data sheets `<Name>` (or `<Name>String`) in the strings folder (`--strings`, default `Strings` next to the schema folder). Empty translations take the base text at build time; format arguments that differ from the base text are warned. Output per language (`Strings/<culture>/<Table>.json`), no generated code for string tables (`--string-keys` adds optional key constants).
- `Ref<StringTable>` accessors return the text in the current language.
- Unreal plugin: per-language string assets and a language list baked by `DrTableBake`; `UDrStringSubsystem` loads only the current language, switches asynchronously, unloads the previous one and fires `OnLanguageChanged`; `UDrLocalizedTextBlock` redraws itself.
- `drtable-gui`: a strings folder setting, and string tables listed separately.
- An input folder with no schema and no data workbook is an error (it used to clear the generated code).

## 0.2.0

First public version.

- `drtable` is a single executable (Rust) with nothing to install.
- Schemas are separate from data: `<Table>.schema.xlsx` in the schema folder, `<Enum>.enum.xlsx` in the enum folder (enum values live in the schema). Generated code depends on the schemas only, so editing data never changes it.
- Data workbooks bind columns by the field names in row 1 (any order). Rows 2-3 are a view of the schema; `drtable new` creates a workbook with reference formulas there. The tool never modifies existing data workbooks.
- Tables can be split over sheets and files (`Items#Weapons`, several workbooks); duplicates are checked across all parts.
- References between tables (`Ref<Table>`, `Ref<Table.SubKey>`), checked by `drtable check` and drawn by `drtable graph`.
- Unreal plugin `DrTableSystem`: baked DataAssets with prebuilt key indices, typed lookups, schema hash checks, `DrTableBake` commandlet with `-Verify`.
- Messages in English or Korean (`--lang`, `DRTABLE_LANG`).
- `drtable-gui`: build and check with an error list that opens the workbook, a table browser, the reference graph, and `new` in a window.
