/// Opaque typed queue payload. The queue policy and validation live in the
/// optional service-authority Prelude; this carrier is shared by JobSpec and
/// every execution tier without importing a service backend.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetJobPayload {
    pub type_id: String,
    pub bytes: Vec<u8>,
    pub publish: bool,
}

/// Typed queue completion payload shared by generated adapters and the queue
/// authority. Its constructor and validation are supplied by ServiceAuthority.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetJobResult {
    pub type_id: String,
    pub bytes: Vec<u8>,
    pub publish: bool,
}

/// Typed queue failure shared by generated adapters and the queue authority.
/// Its constructor and validation are supplied by ServiceAuthority.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetJobError {
    pub type_id: String,
    /// Stable machine-readable reason. Human detail is separate and bounded.
    pub reason: String,
    pub detail: Option<String>,
}
