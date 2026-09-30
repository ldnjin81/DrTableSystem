"""Finding input files and reading sheet names."""

from __future__ import annotations

from pathlib import Path

from .errors import ValidationErrors
from .i18n import tr

# '#' starts a comment everywhere: a sheet named "#Notes" is skipped, and in "Items#Weapons"
# the text from '#' on is a comment, so the sheet is part of table Items.
SHEET_COMMENT = "#"


def strip_sheet_comment(sheet_title: str) -> str:
    """The sheet name without its #comment and surrounding spaces."""
    return sheet_title.split(SHEET_COMMENT, 1)[0].strip()


def table_name_of(sheet_title: str) -> str | None:
    """Table name for a data sheet, or None for note sheets.

    The sheet name without its #comment is the table name. Sheets with the same table name,
    in one file or in several, are parts of one table.
    """
    if sheet_title.startswith("#"):
        return None
    return strip_sheet_comment(sheet_title)


def find_files(
    input_path: Path, suffixes: tuple[str, ...] = (".xlsx",), allow_empty: bool = False,
) -> list[tuple[Path, str]]:
    """Files with one of the suffixes, with their paths relative to the input folder (sorted).

    Excel lock files (~$Book.xlsx) and hidden folders such as .git are skipped.
    """
    if input_path.is_file() and input_path.name.lower().endswith(suffixes):
        return [(input_path, input_path.name)]
    if input_path.is_dir():
        found = []
        for path in input_path.rglob("*"):
            relative = path.relative_to(input_path)
            if (
                path.is_file()
                and path.name.lower().endswith(suffixes)
                and not path.name.startswith("~$")
                and not any(part.startswith(".") for part in relative.parts)
            ):
                found.append((path, relative.as_posix()))
        if found or allow_empty:
            return sorted(found, key=lambda item: item[1])
    raise ValidationErrors([tr(
        f"입력!A1: xlsx 파일을 찾을 수 없습니다: {input_path}",
        f"input!A1: no xlsx file found: {input_path}",
    )])
