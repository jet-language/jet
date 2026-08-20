// ── D-SOA2D: serialization transparency for a columnar list ───────────────────
// `json.to_string` of a `[S]` stored columnar is byte-identical to the
// array-of-structs form, because both encode the same gathered records through
// the one DataTree model. These impls live beside the codec traits (they name
// them) and are Prelude source, so transparency is proven once for every
// columnar struct instead of being re-emitted per struct.
impl<S: JetRow + __jet_Encode> __jet_Encode for JetColumnList<S> {
    fn jet_encode(&self) -> jet_std::DataTree {
        self.to_aos().jet_encode()
    }
}

impl<S: JetRow + __jet_Decode> __jet_Decode for JetColumnList<S> {
    fn jet_decode(tree: &jet_std::DataTree) -> Result<Self, Vec<jet_std::FieldError>> {
        Ok(Self::from_aos(<Vec<S> as __jet_Decode>::jet_decode(tree)?))
    }

    /// D-MIGRATE4: keep the migration report the element type produced — the
    /// layout must not hide that a record arrived as an older published shape.
    fn jet_decode_with_status(
        tree: &jet_std::DataTree,
    ) -> Result<(Self, jet_std::JetMigrationStatus), Vec<jet_std::FieldError>> {
        let (rows, status) = <Vec<S> as __jet_Decode>::jet_decode_with_status(tree)?;
        Ok((Self::from_aos(rows), status))
    }
}
