"""Reference headers: rows 2 and 3 of a data sheet show the schema through Excel formulas.

The build never reads rows 2 and 3, so these formulas are only a view for the people editing
data. ``drtable new`` creates a data workbook with them. The tool never rewrites an existing
data workbook (designers may be editing it); to add the formulas to one, copy rows 2-3 from
another sheet.

Excel keeps a link relative only when the schema file sits in the data file's folder or below
it. Otherwise the link stores the absolute path of whoever saved the workbook, and on another
machine the header keeps showing the values from that save. ``link_warnings`` reports that case.
"""

from __future__ import annotations

import os
import posixpath
import re
import zipfile
from pathlib import Path, PurePosixPath
from urllib.parse import unquote

from openpyxl import Workbook
from openpyxl.packaging.relationship import Relationship
from openpyxl.utils import get_column_letter
from openpyxl.workbook.external_link.external import ExternalBook, ExternalLink, ExternalSheetNames

from .errors import ErrorCollector
from .i18n import tr
from .schema import NAME_ROW, SCOPE_ROW, TYPE_ROW
from .schemafile import SCHEMA_SUFFIXES, Schemas


def new_workbook(target: Path, table: str, schema_root: Path, schemas: Schemas, errors: ErrorCollector) -> bool:
    """Creates a data workbook for table: field names in row 1, reference formulas in rows 2-3.

    Refuses to touch an existing file. Returns True when the workbook was written.
    """
    schema = schemas.tables.get(table)
    if schema is None:
        errors.add(f"[{target.name}]", "A1", tr(
            f"테이블 '{table}'의 스키마가 없습니다", f"table '{table}' has no schema",
        ))
        return False
    if target.exists():
        errors.add(f"[{target.name}]", "A1", tr(
            "이미 있는 파일입니다. drtable은 기존 데이터 파일을 고치지 않습니다",
            "the file already exists; drtable never changes existing data workbooks",
        ))
        return False
    base = schema_root.parent if schema_root.is_file() else schema_root
    target.parent.mkdir(parents=True, exist_ok=True)
    link_target = Path(os.path.relpath((base / schema.file).resolve(), target.parent.resolve())).as_posix()
    workbook = Workbook()
    sheet = workbook.active
    sheet.title = table
    link = ExternalLink(externalBook=ExternalBook(sheetNames=ExternalSheetNames(sheetName=[schema.title or schema.name]), id="rId1"))
    link.file_link = Relationship(type="externalLinkPath", Target=link_target, TargetMode="External", Id="rId1")
    workbook._external_links.append(link)
    reference = f"'[1]{schema.title or schema.name}'"
    missing = tr("(스키마에 없음)", "(not in schema)")
    for column, (_, name, _, _) in enumerate(schema.raw_columns(), start=1):
        letter = get_column_letter(column)
        sheet.cell(NAME_ROW, column).value = str(name)
        for row, source in ((TYPE_ROW, "B"), (SCOPE_ROW, "C")):
            sheet.cell(row, column).value = (
                f"=IFERROR(INDEX({reference}!${source}:${source},"
                f"MATCH({letter}${NAME_ROW},{reference}!$A:$A,0)),\"{missing}\")"
            )
    workbook.save(target)
    return True


def link_warnings(path: Path, relative: str, schema_files: set[Path]) -> list[str]:
    """Warns when a data workbook links to a schema file that is not at the linked path here."""
    try:
        with zipfile.ZipFile(path) as archive:
            names = archive.namelist()
            messages = []
            for name in names:
                if not re.fullmatch(r"xl/externalLinks/externalLink\d+\.xml", name):
                    continue
                book = re.search(rb'<externalBook[^>]*\br:id="([^"]+)"', archive.read(name))
                rels_name = posixpath.join(posixpath.dirname(name), "_rels", posixpath.basename(name) + ".rels")
                if book is None or rels_name not in names:
                    continue
                rels = archive.read(rels_name).decode("utf-8", "replace")
                found = re.search(rf'<Relationship[^>]*Id="{re.escape(book.group(1).decode())}"[^>]*/>', rels)
                target = re.search(r'Target="([^"]+)"', found.group(0)).group(1) if found else ""
                if not target.lower().endswith(SCHEMA_SUFFIXES):
                    continue
                resolved = _resolve(target, path.parent)
                if resolved is None or resolved not in schema_files:
                    messages.append(tr(
                        f"[{relative}]: 참고 헤더가 이 PC에 없는 경로를 가리킵니다({unquote(target)}). "
                        "헤더가 스키마 변경을 따라가지 않습니다. 엑셀의 데이터 > 링크 편집에서 원본을 바꾸거나 drtable new로 만든 파일의 2·3행을 복사해 넣으세요",
                        f"[{relative}]: the reference headers link to a path that does not exist here "
                        f"({unquote(target)}), so they do not follow schema changes; fix it in Excel (Data > Edit Links > Change Source) or paste rows 2-3 from a workbook made by drtable new",
                    ))
            return messages
    except (OSError, zipfile.BadZipFile, KeyError):
        return []


def _resolve(target: str, folder: Path) -> Path | None:
    text = unquote(target)
    if text.lower().startswith("file:///"):
        text = text[len("file:///"):]
    candidate = Path(text.replace("\\", "/"))
    if not re.match(r"^[A-Za-z]:", text) and not text.startswith(("/", "\\")):
        candidate = folder / PurePosixPath(text.replace("\\", "/"))
    try:
        return candidate.resolve() if candidate.exists() else None
    except OSError:
        return None
