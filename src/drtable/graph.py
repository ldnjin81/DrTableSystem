"""Writes table references as a Mermaid diagram in Markdown."""

from __future__ import annotations

from pathlib import Path

from .excel import DataModel


def emit_graph(model: DataModel, output: Path) -> None:
    fence = chr(96) * 3
    lines = ["# Table reference graph", "", fence + "mermaid", "flowchart LR"]
    for table in model.tables:
        key = table.primary_key.type_name
        lines.append(f'    {table.name}["{table.name} ({key})"]')
    for table in model.tables:
        for column in sorted(table.columns, key=lambda item: item.name):
            if column.ref_target is None:
                continue
            label = column.name
            if column.is_array:
                label += f"[{column.array_size}]"
            if column.ref_key:
                label += f" → {column.ref_key} 1:N"
            if column.role == "subkey":
                label += " (SubKey)"
            lines.append(f'    {table.name} -->|"{label}"| {column.ref_target}')
    lines.extend([fence, ""])
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text("\n".join(lines), encoding="utf-8", newline="\n")

