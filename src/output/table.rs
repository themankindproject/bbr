//! Pretty table rendering for humans.

use std::io::IsTerminal;

use comfy_table::{ContentArrangement, Table as ComfyTable};

/// A small wrapper around `comfy_table::Table` that applies our theme and
/// honors `NO_COLOR` / non-TTY.
pub struct Table {
    inner: ComfyTable,
}

impl Table {
    pub fn new() -> Self {
        let mut inner = ComfyTable::new();
        inner
            .load_preset(comfy_table::presets::NOTHING)
            .set_style(comfy_table::TableComponent::HeaderLines, '─')
            .set_content_arrangement(ContentArrangement::Disabled);
        if !std::io::stdout().is_terminal() {
            inner.force_no_tty();
        }
        Self { inner }
    }

    pub fn headers<I, S>(mut self, headers: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let cells: Vec<String> = headers.into_iter().map(Into::into).collect();
        self.inner.set_header(cells.clone());
        for (i, cell) in cells.iter().enumerate() {
            let lower = cell.to_lowercase();
            if lower == "id" {
                if let Some(col) = self.inner.column_mut(i) {
                    col.set_cell_alignment(comfy_table::CellAlignment::Right);
                }
            } else if lower == "state" {
                if let Some(col) = self.inner.column_mut(i) {
                    col.set_cell_alignment(comfy_table::CellAlignment::Center);
                }
            }
        }
        self
    }

    pub fn add_row<I, S>(mut self, row: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let cells: Vec<String> = row.into_iter().map(Into::into).collect();
        self.inner.add_row(cells);
        self
    }

    pub fn render(self) -> String {
        self.inner.to_string()
    }
}

impl Default for Table {
    fn default() -> Self {
        Self::new()
    }
}
