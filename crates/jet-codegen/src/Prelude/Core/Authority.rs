/// D-AUTHORITY-NAME1=A: the one named rights value that crosses boundaries.
/// The rights remain ordinary data; later boundary adapters narrow this set
/// without giving engines a second authority representation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetAuthority {
    rights: std::collections::BTreeSet<String>,
}
