# Changelog

## 0.3.0

- String tables: `<Name>.string.xlsx` lists the languages (code, base mark, scope) and defines the table `<Name>String`; data sheets `<Name>` in the strings folder (`--strings`, default `Strings` next to the schema folder). Empty translations take the base text at build time; format arguments that differ from the base text are warned. Output per language (`Strings/<culture>/<Table>.json`), no generated code for string tables (`--string-keys` adds optional key constants).
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
