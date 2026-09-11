// D-TEST-COMPARE1=A: the named Core carrier lives beside the canonical
// DataTree type, so every generated execution tier sees the same field shape.
#[derive(Clone, Debug)]
pub struct JetTestComparison {
    pub status: String,
    pub relation: String,
    pub source: String,
    pub tool: String,
    pub target: String,
    pub seed: Option<i64>,
    pub case_ids: Vec<String>,
    pub inputs: Vec<DataTree>,
    pub reference: Vec<DataTree>,
    pub candidate: Vec<DataTree>,
    pub first_difference: i64,
    pub reason: String,
    pub universal_proof: bool,
}
