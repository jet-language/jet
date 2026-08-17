/// D-SOA-TIER1=A / D-SOA1: THE shared struct-of-arrays column store.
///
/// A `#Layout(columnar)` struct `S` stores its `[S]` collection column-major:
/// one column per stored field, so a scan of one field walks one contiguous
/// column instead of striding whole records. Every execution tier keeps a
/// columnar list this way and pulls a record out of it through THE ONE read
/// below (`jet_columns_gather`). AOT emit, the Cranelift host and the
/// interpreter ambient differ only in the cell vocabulary `C` they marshal into
/// the columns — never in the layout, the row bookkeeping, the bounds policy, or
/// the read (I9).
///
/// `C` is the tier's cell: `JetVal` for Cranelift, `CtValue` for the ambient,
/// and a generated per-struct cell enum for AOT. Holding the cell abstract is
/// exactly what the ratified tradeoff bought — a tagged cell costs a little
/// speed against a layout tuned per struct, and a tuned column can be added
/// later behind this same read without moving the policy back into an engine.
///
/// The reads take the columns BORROWED (`&[&[C]]`) rather than owned, because a
/// tier whose values live in its own arena or value enum can hand over column
/// slices without copying any rows. `JetColumns` below is the owning store for
/// tiers that hold the columns directly; it delegates to the same free reads, so
/// there is one gather, not one per owner.
/// One record's cells pulled out of a column store, in declaration order.
pub type JetRowCells<C> = Vec<C>;

/// Row count for a borrowed column set. Every column holds the same number of
/// rows, so column 0 answers for all of them; a zero-field record has no column
/// and therefore no rows.
pub fn jet_columns_rows<C>(columns: &[&[C]]) -> usize {
    match columns.first() {
        Some(column) => column.len(),
        None => 0,
    }
}

/// THE read that pulls one record out of a column store.
///
/// Takes a column set and an index, returns that record's cells in declaration
/// order. Bounds selection and wording come from the shared fixed-list stop, so
/// `xs[i]` reports identically whether `xs` is stored columnar or
/// array-of-structs; each tier only maps the returned error onto its own stop.
pub fn jet_columns_gather<C: Clone>(
    columns: &[&[C]],
    index: i64,
) -> Result<JetRowCells<C>, JetFixedListIndexError> {
    jet_fixed_list_index(jet_columns_rows(columns), index, |row| {
        columns.iter().map(|column| column[row].clone()).collect()
    })
}

/// The fused single-field read behind `xs[i].field`: one cell straight out of
/// that field's column, with no whole-record gather. Same store, same bounds
/// policy, same error as the whole-record read above — this is the
/// cache-friendly access the layout exists for.
pub fn jet_columns_gather_cell<C: Clone>(
    columns: &[&[C]],
    field: usize,
    index: i64,
) -> Result<C, JetFixedListIndexError> {
    // Bounds are the row bounds of the whole store, not of one column, so a
    // fused read on a zero-width record still reports the record's row count.
    let rows = jet_columns_rows(columns);
    jet_fixed_list_index(rows, index, |row| columns[field][row].clone())
}

/// The owning column store, for a tier that holds the columns itself.
#[derive(Clone, Debug, PartialEq)]
pub struct JetColumns<C> {
    /// One column per stored field, in declaration order (a computed field is
    /// never a column, D-FIELDPOL1). `push_row` appends to every column
    /// together, which is what keeps their lengths equal.
    cols: Vec<Vec<C>>,
}

impl<C> JetColumns<C> {
    /// An empty store for a record of `width` stored fields.
    pub fn new(width: usize) -> Self {
        Self {
            cols: (0..width).map(|_| Vec::new()).collect(),
        }
    }

    /// Stored-field count — the number of columns.
    pub fn width(&self) -> usize {
        self.cols.len()
    }

    pub fn len(&self) -> usize {
        self.cols.first().map_or(0, Vec::len)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Borrow the columns for the shared reads above.
    pub fn views(&self) -> Vec<&[C]> {
        self.cols.iter().map(Vec::as_slice).collect()
    }

    /// One whole column, for a caller scanning a single field.
    pub fn column(&self, field: usize) -> &[C] {
        match self.cols.get(field) {
            Some(column) => column.as_slice(),
            None => &[],
        }
    }

    /// Scatter one record across the columns — the write side of the layout.
    ///
    /// The row arrives in declaration order, one cell per column. `zip` pairs
    /// each cell with its column, so a row of the wrong width can never index
    /// past a column or panic inside generated code (I2); every tier's
    /// marshalling adapter builds the row from the same field list the columns
    /// were sized from, which is what keeps the widths equal.
    pub fn push_row(&mut self, row: impl IntoIterator<Item = C>) {
        for (column, cell) in self.cols.iter_mut().zip(row) {
            column.push(cell);
        }
    }
}

impl<C: Clone> JetColumns<C> {
    /// THE read, for an owning store: one record's cells at `index`.
    pub fn gather(&self, index: i64) -> Result<JetRowCells<C>, JetFixedListIndexError> {
        jet_columns_gather(&self.views(), index)
    }

    /// The fused single-field read, for an owning store.
    pub fn gather_cell(
        &self,
        field: usize,
        index: i64,
    ) -> Result<C, JetFixedListIndexError> {
        jet_columns_gather_cell(&self.views(), field, index)
    }

    /// Every record in row order — the array-of-structs view of the store.
    /// Iteration, rendering and serialization all read the layout through here,
    /// which is what keeps a columnar list observationally identical to a plain
    /// one (D-SOA2D).
    pub fn rows(&self) -> impl Iterator<Item = JetRowCells<C>> + '_ {
        (0..self.len()).map(move |row| {
            self.cols
                .iter()
                .map(|column| column[row].clone())
                .collect()
        })
    }

    /// Build a store of `width` columns from records in row order.
    pub fn from_rows(width: usize, rows: impl IntoIterator<Item = JetRowCells<C>>) -> Self {
        let mut columns = Self::new(width);
        for row in rows {
            columns.push_row(row);
        }
        columns
    }
}
