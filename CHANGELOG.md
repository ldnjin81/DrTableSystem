# Changelog

## 0.2.0

First public version.

- `drtable` is a single executable (Rust) with nothing to install; the Python implementation in `src/drtable` is the reference the tests compare it with, byte for byte.
- Schemas are separate from data: `<Table>.schema.xlsx` in the schema folder, `<Enum>.enum.xlsx` in the enum folder (enum values live in the schema). Generated code depends on the schemas only, so editing data never changes it.
- Data workbooks bind columns by the field names in row 1 (any order). Rows 2-3 are a view of the schema; `drtable new` creates a workbook with reference formulas there. The tool never modifies existing data workbooks.
- Tables can be split over sheets and files (`Items#Weapons`, several workbooks); duplicates are checked across all parts.
- References between tables (`Ref<Table>`, `Ref<Table.SubKey>`), checked by `drtable check` and drawn by `drtable graph`.
- Unreal plugin `DrTableSystem`: baked DataAssets with prebuilt key indices, typed lookups, schema hash checks, `DrTableBake` commandlet with `-Verify`.
- `drtable migrate` converts the old layout (headers in rows 1-3, `<enum>` sheets) into schema files without touching the data workbooks.
- Messages in English or Korean (`--lang`, `DRTABLE_LANG`).
