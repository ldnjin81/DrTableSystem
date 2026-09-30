//! Writes table references as a Mermaid diagram in Markdown.

use std::path::Path;

use crate::excel::DataModel;

pub fn emit_graph(model: &DataModel, output: &Path) -> std::io::Result<()> {
    let fence = "```";
    let mut lines = vec!["# Table reference graph".to_string(), String::new(), format!("{fence}mermaid"), "flowchart LR".into()];
    for table in &model.tables {
        let key = &table.primary_key().type_name;
        lines.push(format!("    {0}[\"{0} ({key})\"]", table.name));
    }
    for table in &model.tables {
        let mut columns: Vec<_> = table.columns.iter().collect();
        columns.sort_by(|a, b| a.name.cmp(&b.name));
        for column in columns {
            let Some(target) = &column.ref_target else { continue };
            let mut label = column.name.clone();
            if let Some(size) = column.array_size {
                label += &format!("[{size}]");
            }
            if let Some(key) = &column.ref_key {
                label += &format!(" → {key} 1:N");
            }
            if column.is_role("subkey") {
                label += " (SubKey)";
            }
            lines.push(format!("    {} -->|\"{label}\"| {target}", table.name));
        }
    }
    lines.extend([fence.to_string(), String::new()]);
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(output, lines.join("\n"))
}
