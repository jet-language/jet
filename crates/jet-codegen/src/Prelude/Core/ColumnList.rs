/// D-SOA-TIER1=A: the AOT marshalling adapter for one `#Layout(columnar)`
/// record.
///
/// Codegen emits ONE of these per columnar struct and nothing else: a cell
/// vocabulary for the record's fields plus the split/join that moves a record
/// in and out of cells. The storage, the row bookkeeping, the bounds policy and
/// the gather read all live in the shared `JetColumns` store, so the compiled
/// build no longer generates a private per-struct layout (I9: an engine
/// marshals, it does not re-encode policy).
pub trait JetRow: Sized {
    /// The generated cell vocabulary for this record's stored fields — one
    /// variant per column, carrying that field's own Rust type.
    type Cell: Clone;

    /// Stored-field count: how many columns the store needs (a computed field is
    /// never a column, D-FIELDPOL1).
    fn jet_row_width() -> usize;

    /// Split one record into its cells, in declaration order.
    fn jet_row_split(self) -> Vec<Self::Cell>;

    /// Join a record back from its cells, in declaration order.
    fn jet_row_join(cells: Vec<Self::Cell>) -> Self;
}

/// D-SOA1 / D-SOA-TIER1=A: a `[S]` of a `#Layout(columnar)` struct.
///
/// This is the type generated Rust names for a columnar list, and it is Prelude
/// source rather than per-struct emitted code. It holds THE shared column store
/// and does nothing but marshal records into and out of it, so the whole list
/// surface (`len`, `is_empty`, `push`, index-read, field-read, iteration) and
/// the transparency of rendering and serialization are each defined once here
/// instead of re-emitted for every columnar struct.
pub struct JetColumnList<S: JetRow> {
    columns: JetColumns<S::Cell>,
}

impl<S: JetRow> JetColumnList<S> {
    /// An empty columnar list — one empty column per stored field.
    pub fn new() -> Self {
        Self {
            columns: JetColumns::new(S::jet_row_width()),
        }
    }

    pub fn len(&self) -> usize {
        self.columns.len()
    }

    pub fn is_empty(&self) -> bool {
        self.columns.is_empty()
    }

    /// `xs.push(v)` — scatter one record across every column together.
    pub fn push(&mut self, value: S) {
        self.columns.push_row(value.jet_row_split());
    }

    /// `xs[i]` — gather the logical record at `i` through THE shared read, then
    /// join it back into `S`. The bounds stop is the shared list stop, so this
    /// reports identically to an array-of-structs `xs[i]`.
    pub fn gather_at(&self, index: i64, file: &str, line: u32) -> S {
        match self.columns.gather(index) {
            Ok(cells) => S::jet_row_join(cells),
            Err(error) => jet_arithmetic_stop(file, line, &error.message()),
        }
    }

    /// `xs[i].field` — the fused read, straight out of one column, with no
    /// whole-record gather. `field` is the column's declaration-order index.
    pub fn cell(&self, field: usize, index: i64, file: &str, line: u32) -> S::Cell {
        match self.columns.gather_cell(field, index) {
            Ok(cell) => cell,
            Err(error) => jet_arithmetic_stop(file, line, &error.message()),
        }
    }

    /// Build the columns from records in order — what a columnar list literal
    /// lowers to.
    pub fn from_aos(values: Vec<S>) -> Self {
        let mut list = Self::new();
        for value in values {
            list.push(value);
        }
        list
    }

    /// The array-of-structs view. Iteration, rendering and serialization all go
    /// through this, which is what keeps the layout unobservable (D-SOA2D).
    pub fn to_aos(&self) -> Vec<S> {
        self.iter_aos().collect()
    }

    pub fn iter_aos(&self) -> impl Iterator<Item = S> + '_ {
        self.columns.rows().map(S::jet_row_join)
    }
}

impl<S: JetRow> Default for JetColumnList<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: JetRow> Clone for JetColumnList<S> {
    fn clone(&self) -> Self {
        Self {
            columns: self.columns.clone(),
        }
    }
}

impl<S: JetRow> std::fmt::Debug for JetColumnList<S>
where
    S::Cell: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JetColumnList")
            .field("columns", &self.columns)
            .finish()
    }
}

/// D-SOA2D: a columnar list renders exactly like the array-of-structs form, so
/// `print` output never reveals the layout. Proven once here instead of being
/// re-emitted per columnar struct.
impl<S: JetRow + JetShow> JetShow for JetColumnList<S> {
    fn jet_show(&self) -> String {
        self.to_aos().jet_show()
    }
}
