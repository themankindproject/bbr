//! Pretty table rendering for humans.

use comfy_table::{Cell, ColumnConstraint, ContentArrangement, Table as ComfyTable, Width};

/// Maximum character width for free-text columns (title, description, body…).
/// Long strings are truncated by the caller via [`crate::commands::truncate`]
/// before being inserted, so this is a hard safety cap at the table layer.
const MAX_TEXT_COL_WIDTH: u16 = 60;

/// Columns whose header name (case-insensitive) get a width cap applied.
const WIDE_COL_NAMES: &[&str] = &["title", "description", "body", "name", "message", "summary"];

/// A small wrapper around `comfy_table::Table` that applies our theme and
/// honors `NO_COLOR` / non-TTY.
pub struct Table {
    inner: ComfyTable,
}

impl Table {
    pub fn new() -> Self {
        let mut inner = ComfyTable::new();
        let theme = crate::output::theme::Theme::current();
        if theme.unicode_enabled() {
            inner.load_preset(comfy_table::presets::UTF8_FULL);
        } else {
            inner.load_preset(comfy_table::presets::ASCII_FULL);
        }
        inner.set_content_arrangement(ContentArrangement::Disabled);
        if !theme.colors_enabled() {
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
            // Cap wide free-text columns so a single long value can't blow out
            // the terminal width.
            if WIDE_COL_NAMES.iter().any(|&n| lower == n) {
                if let Some(col) = self.inner.column_mut(i) {
                    col.set_constraint(ColumnConstraint::UpperBoundary(Width::Fixed(
                        MAX_TEXT_COL_WIDTH,
                    )));
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
        let cells: Vec<Cell> = row.into_iter().map(|s| Cell::new(s.into())).collect();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_basic_table() {
        let table = Table::new()
            .headers(["Name", "Value"])
            .add_row(["foo", "1"])
            .add_row(["bar", "2"]);
        let out = table.render();
        assert!(out.contains("foo"));
        assert!(out.contains("bar"));
        assert!(out.contains("Name"));
        assert!(out.contains("Value"));
    }

    #[test]
    fn headers_classify_columns() {
        let table = Table::new()
            .headers(["ID", "State", "Name"])
            .add_row(["1", "OPEN", "test"]);
        let out = table.render();
        assert!(out.contains("ID"));
        assert!(out.contains("State"));
        assert!(out.contains("Name"));
    }

    #[test]
    fn empty_table_renders() {
        let table = Table::new().headers(["A", "B"]);
        let out = table.render();
        assert!(out.contains("A"));
        assert!(out.contains("B"));
    }

    #[test]
    fn title_column_constraint_applied() {
        // A title column should have a width constraint set so it doesn't
        // blow out the terminal with a 300-char PR title.
        let table =
            Table::new()
                .headers(["ID", "Title", "State"])
                .add_row(["1", &"x".repeat(200), "OPEN"]);
        let out = table.render();
        // The rendered output must not contain the full 200-char string verbatim
        // (comfy-table will wrap/truncate to the column constraint).
        assert!(!out.contains(&"x".repeat(200)));
    }
}
