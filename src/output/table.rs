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
            .load_preset(comfy_table::presets::UTF8_FULL)
            .apply_modifier(comfy_table::modifiers::UTF8_ROUND_CORNERS)
            .set_content_arrangement(ContentArrangement::Dynamic);
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
        self.inner.set_header(cells);
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
