//! Capability-aware, deterministic table data and rendering.
//!
//! `AdaptiveTable` owns only typed facts.  A borrowed [`OutputProfile`] decides
//! whether a human layout is useful and which width and border vocabulary to
//! use; the machine path stays in [`AdaptiveTable::structured`].

use crate::OutputProfile::OutputProfile;

/// Maximum number of columns retained by one table.
pub const MAX_COLUMNS: usize = 64;
/// Maximum number of rows retained by one table.
pub const MAX_ROWS: usize = 4_096;
/// Maximum UTF-8 bytes retained by one textual cell or column name.
pub const MAX_CELL_BYTES: usize = 4_096;
/// Maximum terminal columns used for one human layout.
pub const MAX_RENDER_WIDTH: usize = 256;

/// The semantic value carried by one table cell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TableCell {
    /// Human-readable text.
    Text(String),
    /// A signed number, rendered without locale-dependent formatting.
    Integer(i128),
    /// An unsigned number, rendered without locale-dependent formatting.
    Unsigned(u128),
    /// A boolean fact.
    Boolean(bool),
    /// A deliberately absent value.
    Empty,
}

impl TableCell {
    /// Construct a bounded textual cell with terminal controls removed.
    pub fn text(value: impl Into<String>) -> Self {
        let value = value.into();
        Self::Text(bounded_text(&value))
    }

    /// Construct a signed numeric cell.
    pub const fn integer(value: i128) -> Self {
        Self::Integer(value)
    }

    /// Construct an unsigned numeric cell.
    pub const fn unsigned(value: u128) -> Self {
        Self::Unsigned(value)
    }

    /// Construct a boolean cell.
    pub const fn boolean(value: bool) -> Self {
        Self::Boolean(value)
    }

    /// Construct an empty cell.
    pub const fn empty() -> Self {
        Self::Empty
    }

    /// Return the schema kind represented by this value.
    pub const fn kind(&self) -> TableCellKind {
        match self {
            Self::Text(_) => TableCellKind::Text,
            Self::Integer(_) => TableCellKind::Integer,
            Self::Unsigned(_) => TableCellKind::Unsigned,
            Self::Boolean(_) => TableCellKind::Boolean,
            Self::Empty => TableCellKind::Empty,
        }
    }

    fn display_text(&self) -> String {
        match self {
            Self::Text(value) => bounded_text(value),
            Self::Integer(value) => value.to_string(),
            Self::Unsigned(value) => value.to_string(),
            Self::Boolean(value) => value.to_string(),
            Self::Empty => String::new(),
        }
    }
}

/// The declared kind of a table column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableCellKind {
    /// Free-form text.
    Text,
    /// Signed numeric values.
    Integer,
    /// Unsigned numeric values.
    Unsigned,
    /// Boolean values.
    Boolean,
    /// A column whose values are intentionally absent.
    Empty,
}

impl TableCellKind {
    const fn default_alignment(self) -> TableAlignment {
        match self {
            Self::Integer | Self::Unsigned => TableAlignment::Right,
            Self::Text | Self::Boolean | Self::Empty => TableAlignment::Left,
        }
    }
}

/// Alignment applied to cells in one column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableAlignment {
    /// Pad on the right.
    Left,
    /// Pad on the left.
    Right,
    /// Split extra space as evenly as possible.
    Center,
}

/// A bounded table column schema.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableColumn {
    /// Stable human-facing heading.
    pub name: String,
    /// Declared value kind.
    pub kind: TableCellKind,
    /// Human layout alignment.
    pub alignment: TableAlignment,
}

impl TableColumn {
    /// Construct one column and infer its natural alignment from its kind.
    pub fn new(name: impl Into<String>, kind: TableCellKind) -> Self {
        let name = name.into();
        Self {
            name: bounded_text(&name),
            kind,
            alignment: kind.default_alignment(),
        }
    }

    /// Override the inferred alignment for a deliberate presentation choice.
    pub fn with_alignment(mut self, alignment: TableAlignment) -> Self {
        self.alignment = alignment;
        self
    }
}

/// One bounded row of typed cells.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableRow {
    /// Cells in declaration order.  Extra cells are ignored by a table whose
    /// schema has fewer columns.
    pub cells: Vec<TableCell>,
}

impl TableRow {
    /// Construct a row, retaining at most [`MAX_COLUMNS`] cells.
    pub fn new(mut cells: Vec<TableCell>) -> Self {
        cells.truncate(MAX_COLUMNS);
        Self { cells }
    }
}

/// The machine-readable projection of a table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StructuredTable {
    /// The bounded schema used by the source table.
    pub columns: Vec<TableColumn>,
    /// The bounded typed rows, in source order.
    pub rows: Vec<TableRow>,
    /// Whether input columns, rows, or cells were dropped at a bound.
    pub truncated: bool,
}

impl StructuredTable {
    fn new(mut columns: Vec<TableColumn>, mut rows: Vec<TableRow>, mut truncated: bool) -> Self {
        if columns.len() > MAX_COLUMNS {
            columns.truncate(MAX_COLUMNS);
            truncated = true;
        }
        if rows.len() > MAX_ROWS {
            rows.truncate(MAX_ROWS);
            truncated = true;
        }
        for row in &mut rows {
            if row.cells.len() > columns.len() {
                row.cells.truncate(columns.len());
                truncated = true;
            }
        }
        Self {
            columns,
            rows,
            truncated,
        }
    }
}

/// A table whose facts are separated from its human layout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdaptiveTable {
    columns: Vec<TableColumn>,
    rows: Vec<TableRow>,
    truncated: bool,
}

impl AdaptiveTable {
    /// Construct a bounded table from a schema and typed rows.
    pub fn new(mut columns: Vec<TableColumn>, mut rows: Vec<TableRow>) -> Self {
        let mut truncated = false;
        if columns.len() > MAX_COLUMNS {
            columns.truncate(MAX_COLUMNS);
            truncated = true;
        }
        if rows.len() > MAX_ROWS {
            rows.truncate(MAX_ROWS);
            truncated = true;
        }
        for column in &mut columns {
            column.name = bounded_text(&column.name);
        }
        for row in &mut rows {
            if row.cells.len() > columns.len() {
                row.cells.truncate(columns.len());
                truncated = true;
            }
            for cell in &mut row.cells {
                if let TableCell::Text(value) = cell {
                    *value = bounded_text(value);
                }
            }
        }
        Self {
            columns,
            rows,
            truncated,
        }
    }

    /// Return the bounded schema.
    pub fn columns(&self) -> &[TableColumn] {
        &self.columns
    }

    /// Return the bounded source rows.
    pub fn rows(&self) -> &[TableRow] {
        &self.rows
    }

    /// Report whether a constructor bound discarded input.
    pub const fn truncated(&self) -> bool {
        self.truncated
    }

    /// Build a deterministic human layout using the supplied capability
    /// profile.  The profile is borrowed and no environment is read here.
    pub fn layout(&self, profile: &OutputProfile) -> TableLayout<'_> {
        let width = profile.width().clamp(1, MAX_RENDER_WIDTH);
        let column_count = self.columns.len();
        let natural = self.natural_widths();
        let minimum_grid_width = column_count
            .saturating_mul(4)
            .saturating_add(1);
        let plain = column_count == 0 || (column_count == 1 && width < 5);
        let narrow = !plain && column_count > 1 && width < minimum_grid_width;
        let widths = if plain || narrow {
            Vec::new()
        } else {
            fit_widths(&natural, width)
        };
        TableLayout {
            table: self,
            widths,
            width,
            unicode: profile.unicode_enabled(),
            human: profile.human_enabled(),
            narrow,
            plain,
        }
    }

    /// Return the bounded typed facts for machine output.
    pub fn structured(&self) -> StructuredTable {
        StructuredTable::new(
            self.columns.clone(),
            self.rows.clone(),
            self.truncated,
        )
    }

    fn natural_widths(&self) -> Vec<usize> {
        self.columns
            .iter()
            .enumerate()
            .map(|(index, column)| {
                let header = display_width(&column.name);
                let cells = self
                    .rows
                    .iter()
                    .filter_map(|row| row.cells.get(index))
                    .map(|cell| display_width(&cell.display_text()))
                    .max()
                    .unwrap_or(0);
                header.max(cells).clamp(1, MAX_RENDER_WIDTH)
            })
            .collect()
    }
}

/// A computed layout.  Rendering is human-only; machine callers use
/// [`AdaptiveTable::structured`] instead of parsing padding or borders.
pub struct TableLayout<'a> {
    table: &'a AdaptiveTable,
    widths: Vec<usize>,
    width: usize,
    unicode: bool,
    human: bool,
    narrow: bool,
    plain: bool,
}

impl<'a> TableLayout<'a> {
    /// Return the fitted content width for every grid column.
    pub fn widths(&self) -> &[usize] {
        &self.widths
    }

    /// Return the requested width after the renderer's safety cap.
    pub const fn width(&self) -> usize {
        self.width
    }

    /// Return true when the grid was replaced by a label/value layout.
    pub const fn is_narrow(&self) -> bool {
        self.narrow
    }

    /// Render the layout when the profile permits human output.
    ///
    /// Machine and otherwise hidden channels return `None`, never an ANSI or
    /// progress fragment.  Human rendering itself never emits ANSI.
    pub fn render(&self) -> Option<String> {
        if !self.human {
            return None;
        }
        Some(if self.plain {
            render_plain(self.table, self.width)
        } else if self.narrow {
            render_narrow(self.table, self.width)
        } else {
            render_grid(self.table, &self.widths, self.unicode)
        })
    }
}

fn fit_widths(natural: &[usize], width: usize) -> Vec<usize> {
    if natural.is_empty() {
        return Vec::new();
    }
    let overhead = natural.len().saturating_mul(3).saturating_add(1);
    let budget = width.saturating_sub(overhead);
    if budget < natural.len() {
        return vec![1; natural.len()];
    }
    let mut widths = vec![1usize; natural.len()];
    let mut remaining = budget - natural.len();
    while remaining != 0 {
        let mut grew = false;
        for (index, current) in widths.iter_mut().enumerate() {
            if *current < natural[index] {
                *current += 1;
                remaining -= 1;
                grew = true;
                if remaining == 0 {
                    break;
                }
            }
        }
        if !grew {
            break;
        }
    }
    widths
}

fn render_grid(table: &AdaptiveTable, widths: &[usize], unicode: bool) -> String {
    let (left, joint, right, fill, separator) = if unicode {
        ('┌', '┬', '┐', '─', '│')
    } else {
        ('+', '+', '+', '-', '|')
    };
    let (mid_left, mid_joint, mid_right) = if unicode {
        ('├', '┼', '┤')
    } else {
        ('+', '+', '+')
    };
    let (bottom_left, bottom_joint, bottom_right) = if unicode {
        ('└', '┴', '┘')
    } else {
        ('+', '+', '+')
    };
    let mut lines = vec![border(widths, left, joint, right, fill)];
    if !table.columns.is_empty() {
        let headers = table
            .columns
            .iter()
            .map(|column| TableCell::text(column.name.clone()))
            .collect::<Vec<_>>();
        lines.extend(render_grid_row(
            &headers,
            &table.columns,
            widths,
            separator,
        ));
        lines.push(border(widths, mid_left, mid_joint, mid_right, fill));
    }
    for row in &table.rows {
        lines.extend(render_grid_row(
            &row.cells,
            &table.columns,
            widths,
            separator,
        ));
    }
    lines.push(border(
        widths,
        bottom_left,
        bottom_joint,
        bottom_right,
        fill,
    ));
    lines.join("\n")
}

fn border(widths: &[usize], left: char, joint: char, right: char, fill: char) -> String {
    let mut line = String::new();
    line.push(left);
    for (index, width) in widths.iter().enumerate() {
        if index != 0 {
            line.push(joint);
        }
        line.extend(std::iter::repeat(fill).take(width.saturating_add(2)));
    }
    line.push(right);
    line
}

fn render_grid_row(
    cells: &[TableCell],
    columns: &[TableColumn],
    widths: &[usize],
    separator: char,
) -> Vec<String> {
    let wrapped = widths
        .iter()
        .enumerate()
        .map(|(index, width)| {
            let value = cells
                .get(index)
                .map(TableCell::display_text)
                .unwrap_or_default();
            wrap_cell(&value, *width)
        })
        .collect::<Vec<_>>();
    let height = wrapped.iter().map(Vec::len).max().unwrap_or(1);
    (0..height)
        .map(|line_index| {
            let mut line = String::new();
            line.push(separator);
            for (index, width) in widths.iter().enumerate() {
                if index != 0 {
                    line.push(separator);
                }
                let value = wrapped[index]
                    .get(line_index)
                    .map(String::as_str)
                    .unwrap_or_default();
                line.push(' ');
                line.push_str(&pad_cell(
                    value,
                    *width,
                    columns
                        .get(index)
                        .map(|column| column.alignment)
                        .unwrap_or(TableAlignment::Left),
                ));
                line.push(' ');
            }
            line.push(separator);
            line
        })
        .collect()
}

fn render_narrow(table: &AdaptiveTable, width: usize) -> String {
    let label_width = table
        .columns
        .iter()
        .map(|column| display_width(&column.name))
        .max()
        .unwrap_or(0)
        .min(width.saturating_sub(3));
    let value_width = width.saturating_sub(label_width.saturating_add(2)).max(1);
    let mut lines = Vec::new();
    for row in &table.rows {
        for (index, column) in table.columns.iter().enumerate() {
            let label = clip_to_width(&column.name, label_width);
            let value = row
                .cells
                .get(index)
                .map(TableCell::display_text)
                .unwrap_or_default();
            let wrapped = wrap_cell(&value, value_width);
            for (line_index, piece) in wrapped.iter().enumerate() {
                let mut line = String::new();
                if line_index == 0 {
                    line.push_str(&pad_cell(&label, label_width, TableAlignment::Left));
                    if label_width != 0 {
                        line.push_str(": ");
                    }
                } else if label_width != 0 {
                    line.extend(std::iter::repeat(' ').take(label_width + 2));
                }
                line.push_str(piece);
                lines.push(clip_to_width(&line, width));
            }
        }
    }
    if lines.is_empty() && !table.columns.is_empty() {
        lines.push(clip_to_width(
            &table
                .columns
                .iter()
                .map(|column| column.name.as_str())
                .collect::<Vec<_>>()
                .join(" | "),
            width,
        ));
    }
    lines.join("\n")
}

fn render_plain(table: &AdaptiveTable, width: usize) -> String {
    let mut lines = Vec::new();
    if !table.columns.is_empty() {
        let heading = table
            .columns
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>()
            .join(" | ");
        lines.extend(wrap_cell(&heading, width));
    }
    for row in &table.rows {
        let values = row
            .cells
            .iter()
            .map(TableCell::display_text)
            .collect::<Vec<_>>()
            .join(" | ");
        lines.extend(wrap_cell(&values, width));
    }
    lines.join("\n")
}

fn pad_cell(value: &str, width: usize, alignment: TableAlignment) -> String {
    let value = clip_to_width(value, width);
    let gap = width.saturating_sub(display_width(&value));
    match alignment {
        TableAlignment::Left => format!("{value}{}", " ".repeat(gap)),
        TableAlignment::Right => format!("{}{}", " ".repeat(gap), value),
        TableAlignment::Center => {
            let left = gap / 2;
            format!("{}{value}{}", " ".repeat(left), " ".repeat(gap - left))
        }
    }
}

fn wrap_cell(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let text = bounded_text(text);
    let mut lines = Vec::new();
    for logical in text.split('\n') {
        let logical = logical.strip_suffix('\r').unwrap_or(logical);
        if logical.is_empty() {
            lines.push(String::new());
            continue;
        }
        let mut line = String::new();
        let mut line_width = 0usize;
        let mut emitted = false;
        for_each_cluster(logical, |cluster, cluster_width| {
            if cluster_width == 0 {
                if line_width < width {
                    line.push_str(cluster);
                }
                return;
            }
            if cluster_width > width {
                if !line.is_empty() {
                    lines.push(std::mem::take(&mut line));
                    line_width = 0;
                }
                lines.push("?".to_string());
                emitted = true;
                return;
            }
            if line_width != 0 && line_width.saturating_add(cluster_width) > width {
                lines.push(std::mem::take(&mut line));
                line_width = 0;
            }
            line.push_str(cluster);
            line_width = line_width.saturating_add(cluster_width);
        });
        if !line.is_empty() || !emitted {
            lines.push(line);
        }
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}
pub(crate) fn clip_to_width_with_unicode(text: &str, width: usize, unicode: bool) -> String {
    if width == 0 {
        return String::new();
    }
    let text = bounded_text(text);
    if display_width(&text) <= width {
        return text;
    }
    let suffix = if unicode { "…" } else { "..." };
    let suffix_width = display_width(suffix);
    if width <= suffix_width {
        return suffix.chars().take(width).collect();
    }
    let keep = width - suffix_width;
    let mut out = String::new();
    let mut used = 0usize;
    let mut stopped = false;
    for_each_cluster(&text, |cluster, cluster_width| {
        if stopped {
            return;
        }
        if used >= keep {
            stopped = true;
            return;
        }
        if cluster_width == 0 {
            out.push_str(cluster);
            return;
        }
        if cluster_width > keep.saturating_sub(used) {
            stopped = true;
            return;
        }
        out.push_str(cluster);
        used = used.saturating_add(cluster_width);
    });
    out.push_str(suffix);
    out
}

fn clip_to_width(text: &str, width: usize) -> String {
    clip_to_width_with_unicode(text, width, true)
}


pub(crate) fn display_width(text: &str) -> usize {
    strip_ansi(text)
        .split('\n')
        .map(|line| {
            let mut width = 0usize;
            for_each_cluster(line, |_, cluster_width| {
                width = width.saturating_add(cluster_width);
            });
            width
        })
        .max()
        .unwrap_or(0)
}

fn bounded_text(text: &str) -> String {
    let text = strip_ansi(text);
    if text.len() <= MAX_CELL_BYTES {
        return text;
    }
    let mut out = String::with_capacity(MAX_CELL_BYTES);
    let mut bytes = 0usize;
    for_each_cluster(&text, |cluster, _| {
        if bytes.saturating_add(cluster.len()) <= MAX_CELL_BYTES {
            out.push_str(cluster);
            bytes += cluster.len();
        }
    });
    out
}
fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut state = 0u8;
    for character in text.chars() {
        match state {
            0 if character == '\x1b' => state = 1,
            0 => out.push(character),
            1 if character == '[' => state = 2,
            1 if character == ']' => state = 3,
            1 if character.is_ascii_alphabetic() => state = 0,
            1 => state = 0,
            2 if ('@'..='~').contains(&character) => state = 0,
            2 => {}
            3 if character == '\x07' => state = 0,
            3 if character == '\x1b' => state = 4,
            3 => {}
            4 if character == '\\' => state = 0,
            4 => state = 3,
            _ => state = 0,
        }
    }
    out
}

fn for_each_cluster(text: &str, mut visit: impl FnMut(&str, usize)) {
    let mut chars = text.char_indices().peekable();
    while let Some((start, first)) = chars.next() {
        let mut end = start + first.len_utf8();
        let mut width = scalar_width(first);
        if is_regional_indicator(first) {
            if let Some((_, next)) = chars.peek().copied() {
                if is_regional_indicator(next) {
                    if let Some((next_start, next)) = chars.next() {
                        end = next_start + next.len_utf8();
                        width = 2;
                    }
                }
            }
        } else {
            loop {
                while let Some((_, next)) = chars.peek().copied() {
                    if is_extend(next) {
                        let (next_start, next) = chars.next().expect("peeked cluster extension");
                        end = next_start + next.len_utf8();
                    } else {
                        break;
                    }
                }
                let Some((_, '\u{200d}')) = chars.peek().copied() else {
                    break;
                };
                let (join_start, joiner) = chars.next().expect("peeked grapheme joiner");
                end = join_start + joiner.len_utf8();
                let Some((next_start, next)) = chars.next() else {
                    break;
                };
                end = next_start + next.len_utf8();
                width = width.max(scalar_width(next));
            }
        }
        visit(&text[start..end], width);
    }
}

fn scalar_width(character: char) -> usize {
    if character.is_control() || is_extend(character) || character == '\u{200d}' {
        0
    } else if is_wide(character) {
        2
    } else {
        1
    }
}

fn is_regional_indicator(character: char) -> bool {
    matches!(character as u32, 0x1f1e6..=0x1f1ff)
}

fn is_extend(character: char) -> bool {
    matches!(
        character as u32,
        0x0300..=0x036f
            | 0x0483..=0x0489
            | 0x0591..=0x05bd
            | 0x05bf
            | 0x05c1..=0x05c2
            | 0x05c4..=0x05c5
            | 0x0610..=0x061a
            | 0x064b..=0x065f
            | 0x0670
            | 0x06d6..=0x06ed
            | 0x0711
            | 0x0730..=0x074a
            | 0x07a6..=0x07b0
            | 0x07eb..=0x07f3
            | 0x0816..=0x0819
            | 0x081b..=0x0823
            | 0x0825..=0x0827
            | 0x0829..=0x082d
            | 0x0859..=0x085b
            | 0x08d3..=0x0903
            | 0x093a..=0x093c
            | 0x093e..=0x094f
            | 0x0951..=0x0957
            | 0x0962..=0x0963
            | 0x0981..=0x0984
            | 0x09be..=0x09c4
            | 0x09c7..=0x09c8
            | 0x09cb..=0x09cd
            | 0x09d7
            | 0x09e2..=0x09e3
            | 0x0a01..=0x0a03
            | 0x0a3c
            | 0x0a3e..=0x0a42
            | 0x0a47..=0x0a48
            | 0x0a4b..=0x0a4d
            | 0x0a51
            | 0x0a70..=0x0a71
            | 0x0a75
            | 0x0abc
            | 0x0abe..=0x0ac5
            | 0x0ac7..=0x0ac9
            | 0x0acb..=0x0acd
            | 0x0ae2..=0x0ae3
            | 0x0b01..=0x0b03
            | 0x0b3c
            | 0x0b3e..=0x0b44
            | 0x0b47..=0x0b48
            | 0x0b4b..=0x0b4d
            | 0x0b56..=0x0b57
            | 0x0b62..=0x0b63
            | 0x0c00..=0x0c04
            | 0x0c3e..=0x0c44
            | 0x0c46..=0x0c48
            | 0x0c4a..=0x0c4d
            | 0x0c55..=0x0c56
            | 0x0c62..=0x0c63
            | 0x0d00..=0x0d03
            | 0x0d3b..=0x0d44
            | 0x0d46..=0x0d48
            | 0x0d4a..=0x0d4d
            | 0x0d57
            | 0x0d62..=0x0d63
            | 0x0e31
            | 0x0e34..=0x0e3a
            | 0x0e47..=0x0e4e
            | 0x0eb1
            | 0x0eb4..=0x0ebc
            | 0x0ec8..=0x0ecd
            | 0x0f18
            | 0x0f35
            | 0x0f37
            | 0x0f39
            | 0x0f71..=0x0f84
            | 0x0f86..=0x0f87
            | 0x0f8d..=0x0fbc
            | 0x0fc6
            | 0x102b..=0x103e
            | 0x1056..=0x1059
            | 0x105e..=0x1060
            | 0x1062..=0x1064
            | 0x1067..=0x106d
            | 0x1071..=0x1074
            | 0x1082
            | 0x1084
            | 0x1087..=0x108d
            | 0x108f
            | 0x109a..=0x109d
            | 0x135d..=0x135f
            | 0x1712..=0x1714
            | 0x1732..=0x1734
            | 0x1752..=0x1753
            | 0x1772..=0x1773
            | 0x17b4..=0x17d3
            | 0x17dd
            | 0x180b..=0x180f
            | 0x1885..=0x1886
            | 0x18a9
            | 0x1920..=0x193b
            | 0x1a17..=0x1a1b
            | 0x1a55..=0x1a5e
            | 0x1a60
            | 0x1a62..=0x1a6f
            | 0x1a70..=0x1a7c
            | 0x1a7f
            | 0x1ab0..=0x1aff
            | 0x1b00..=0x1b04
            | 0x1b34
            | 0x1b36..=0x1b44
            | 0x1b6b..=0x1b73
            | 0x1b80..=0x1b82
            | 0x1ba1..=0x1bad
            | 0x1be6..=0x1bf3
            | 0x1c24..=0x1c37
            | 0x1cd0..=0x1ce8
            | 0x1ced
            | 0x1cf4
            | 0x1cf7..=0x1cfc
            | 0x1d167..=0x1d1ff
            | 0x1dc00..=0x1dfff
            | 0x20d0..=0x20f0
            | 0x2cef..=0x2cf1
            | 0x2de0..=0x2dff
            | 0x302a..=0x302f
            | 0xa66f..=0xa672
            | 0xa674..=0xa67d
            | 0xa69e..=0xa69f
            | 0xa6f0..=0xa6f1
            | 0xa802
            | 0xa806
            | 0xa80b
            | 0xa823..=0xa827
            | 0xa82c
            | 0xa880..=0xa881
            | 0xa8b4..=0xa8c5
            | 0xa8e0..=0xa8f1
            | 0xa8ff
            | 0xa926..=0xa92f
            | 0xa947..=0xa953
            | 0xa980..=0xa983
            | 0xa9b3..=0xa9c0
            | 0xa9c6
            | 0xa9e5
            | 0xaa29..=0xaa3f
            | 0xaa43
            | 0xaa4c..=0xaa4d
            | 0xaa7b..=0xaa7d
            | 0xaab0
            | 0xaab2..=0xaab4
            | 0xaab7..=0xaab8
            | 0xaabe..=0xaabf
            | 0xaac1
            | 0xaaeb..=0xaaef
            | 0xaaf5..=0xaaf6
            | 0xabe3..=0xabe4
            | 0xabe6..=0xabe7
            | 0xabe9
            | 0xabec
            | 0xabf0..=0xabf1
            | 0xfe00..=0xfe0f
            | 0xfe20..=0xfe2f
            | 0x1f3fb..=0x1f3ff
            | 0x200b
            | 0x200c
            | 0x200e..=0x200f
            | 0x2060..=0x2064
            | 0x2066..=0x206f
    )
}

fn is_wide(character: char) -> bool {
    matches!(
        character as u32,
        0x1100..=0x115f
            | 0x231a..=0x231b
            | 0x2329..=0x232a
            | 0x23e9..=0x23ec
            | 0x23f0
            | 0x23f3
            | 0x25fd..=0x25fe
            | 0x2614..=0x2615
            | 0x2648..=0x2653
            | 0x267f
            | 0x2693
            | 0x26a1
            | 0x26aa..=0x26ab
            | 0x26bd..=0x26be
            | 0x26c4..=0x26c5
            | 0x26ce
            | 0x26d4
            | 0x26ea
            | 0x26f2..=0x26f3
            | 0x26f5
            | 0x26fa
            | 0x26fd
            | 0x2705
            | 0x270a..=0x270b
            | 0x2728
            | 0x274c
            | 0x274e
            | 0x2753..=0x2755
            | 0x2757
            | 0x2795..=0x2797
            | 0x27b0
            | 0x27bf
            | 0x2b1b..=0x2b1c
            | 0x2b50
            | 0x2b55
            | 0x2e80..=0x2ffb
            | 0x3000..=0x303e
            | 0x3041..=0x3247
            | 0x3250..=0x4dbf
            | 0x4e00..=0xa4c6
            | 0xa960..=0xa97c
            | 0xac00..=0xd7a3
            | 0xf900..=0xfaff
            | 0xfe10..=0xfe19
            | 0xfe30..=0xfe6b
            | 0xff01..=0xff60
            | 0xffe0..=0xffe6
            | 0x1f300..=0x1f64f
            | 0x1f680..=0x1f6ff
            | 0x1f900..=0x1f9ff
            | 0x20000..=0x3fffd
    )
}
