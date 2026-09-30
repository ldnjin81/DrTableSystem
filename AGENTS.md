# Working on DrTableSystem

Rules for contributors and coding agents.

## Layout

| Path | What |
|---|---|
| `rust/` | The `drtable` executable and the `drtable-gui` window (`--features gui`), sharing one library. |
| `src/drtable/` | Python reference implementation of the same behaviour. |
| `tests/` | One test suite for both implementations. |
| `unreal/DrTableSystem/` | Unreal Engine plugin (runtime registry, bake commandlet, tests). |
| `docs/en`, `docs/ko` | User manuals. The manual is the specification. |

## Rules

- **Change both implementations together.** Any change in behaviour, output or messages goes into `rust/` and `src/drtable/` in the same commit.
- **Byte-for-byte parity.** `DRTABLE_BIN=<path to drtable> uv run pytest -q` runs every test against the executable; each `build`, `graph` and `check` call also runs the Python implementation and must produce the same exit code, stdout, stderr and files.
- **Deterministic output.** The same input gives the same bytes: no dependence on hash or directory order, LF line endings, fixed JSON key order, no timestamps unless `--stamp` is given.
- **Generated code depends on the schemas only**, never on data values or data file names.
- **The tool never modifies existing data workbooks.** It only creates new files (`migrate`, `new`).
- Every error message starts with a `[File]Sheet!Cell` location, and exists in English and Korean (`tr(ko, en)`).
- Test workbooks are created inside the tests with openpyxl; no binary fixtures in the repository.
- Keep dependencies minimal: Python `openpyxl`; Rust `calamine`, `rust_xlsxwriter`, `zip`, `sha2`, `serde_json`, `regex`, `rayon`, `chrono`. Explain any addition in the commit message.

- The GUI only calls the library; it has no behaviour of its own to keep in parity. Regenerate the manual screenshots with `drtable-screenshots` (`--features screenshots`, renders without a window).

## Commands

```sh
uv sync && uv run ruff check src tests && uv run pytest -q     # reference implementation
cd rust && cargo build --release && cargo test --release       # executable
DRTABLE_BIN=$PWD/rust/target/release/drtable uv run pytest -q  # parity
```
