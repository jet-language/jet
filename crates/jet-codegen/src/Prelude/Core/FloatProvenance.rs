/// D-FLOAT-ORIGIN / I9: the one rendering rule for tracked-float provenance.
///
/// TIR supplies the provenance fact. Backends only marshal the optional note
/// into this operation and carry the returned String to their own boundary.
pub fn jet_float_origin(origin: Option<&str>) -> String {
    origin.unwrap_or("untracked").to_string()
}
