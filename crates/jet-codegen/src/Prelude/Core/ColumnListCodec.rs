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
}
