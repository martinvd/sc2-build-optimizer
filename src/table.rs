//! Minimal box-drawn text tables for terminal output.

#[derive(Clone, Copy)]
pub enum Align {
    Left,
    Right,
}

pub struct Table {
    headers: Vec<&'static str>,
    aligns: Vec<Align>,
    rows: Vec<Vec<String>>,
}

impl Table {
    pub fn new(columns: &[(&'static str, Align)]) -> Self {
        Table {
            headers: columns.iter().map(|c| c.0).collect(),
            aligns: columns.iter().map(|c| c.1).collect(),
            rows: Vec::new(),
        }
    }

    pub fn row(&mut self, cells: Vec<String>) {
        assert_eq!(cells.len(), self.headers.len(), "row width must match the header");
        self.rows.push(cells);
    }

    pub fn render(&self) -> String {
        let width = |s: &str| s.chars().count();
        let widths: Vec<usize> = (0..self.headers.len())
            .map(|i| self.rows.iter().map(|r| width(&r[i])).chain([width(self.headers[i])]).max().unwrap())
            .collect();

        let rule = |left: &str, mid: &str, right: &str| {
            let segments: Vec<String> = widths.iter().map(|w| "─".repeat(w + 2)).collect();
            format!("{left}{}{right}\n", segments.join(mid))
        };
        let line = |cells: &[&str], aligns: &[Align]| {
            let padded: Vec<String> = cells
                .iter()
                .zip(&widths)
                .zip(aligns)
                .map(|((cell, &w), align)| match align {
                    Align::Left => format!(" {cell:<w$} "),
                    Align::Right => format!(" {cell:>w$} "),
                })
                .collect();
            format!("│{}│\n", padded.join("│"))
        };

        let mut out = rule("┌", "┬", "┐");
        out += &line(&self.headers, &vec![Align::Left; widths.len()]);
        out += &rule("├", "┼", "┤");
        for row in &self.rows {
            let cells: Vec<&str> = row.iter().map(String::as_str).collect();
            out += &line(&cells, &self.aligns);
        }
        out + &rule("└", "┴", "┘")
    }
}
