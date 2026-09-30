# Working on DrTableSystem

Rules for contributors and coding agents.

## Layout

| Path | What |
|---|---|
| `rust/` | The `drtable` executable and the `drtable-gui` window (`--features gui`), sharing one library. |
| `tests/` | Black-box tests of the executable (Python + openpyxl build the input workbooks). |
| `unreal/DrTableSystem/` | Unreal Engine plugin (runtime registry, bake commandlet, tests). |
| `docs/en`, `docs/ko` | User manuals. The manual is the specification. |

## Rules

- **Deterministic output.** The same input gives the same bytes: no dependence on hash or directory order, LF line endings, fixed JSON key order, no timestamps unless `--stamp` is given.
- **Generated code depends on the schemas only**, never on data values or data file names.
- **The tool never modifies existing data workbooks.** It only creates new files (`new`).
- Every error message starts with a `[File]Sheet!Cell` location, and exists in English and Korean (`tr(ko, en)`).
- Every behaviour change comes with a test in `tests/`. Test workbooks are created inside the tests with openpyxl; no binary fixtures in the repository.
- Keep dependencies minimal: `calamine`, `rust_xlsxwriter`, `zip`, `sha2`, `serde_json`, `regex`, `rayon`, `chrono` (GUI: `eframe`, `rfd`, `serde`, `png`). Explain any addition in the commit message.
- The GUI only calls the library. Regenerate the manual screenshots with `drtable-screenshots` (`--features screenshots`, renders without a window).

## Commands

```sh
cd rust && cargo build --release && cargo test --release    # executable
uv sync && uv run ruff check tests && uv run pytest -q     # test suite (builds the executable if needed)
```
