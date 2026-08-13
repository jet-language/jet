/// D-BOUND-EVOLVE1=A: the one ordered-wire merge used by every execution tier.
/// Known values replace their old positions; new known values append in schema
/// order; unknown entries keep their original positions and values.
pub fn jet_wire_order_merge<T: Clone>(
    known: &[(String, T)],
    original: &[(String, T)],
) -> Vec<(String, T)> {
    let mut out = Vec::with_capacity(known.len().max(original.len()));
    let mut emitted = Vec::new();
    for (key, value) in original {
        if let Some((_, current)) = known.iter().find(|(known_key, _)| known_key == key) {
            out.push((key.clone(), current.clone()));
            emitted.push(key.clone());
        } else {
            out.push((key.clone(), value.clone()));
        }
    }
    for (key, value) in known {
        if !emitted.iter().any(|emitted_key| emitted_key == key) {
            out.push((key.clone(), value.clone()));
        }
    }
    out
}
