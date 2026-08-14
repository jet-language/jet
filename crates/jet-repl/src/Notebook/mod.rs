//! Notebook kernel, document, trust, and headless Jupyter protocol
//! (D-NOTEBOOK-SURFACE1=D, D-NOTEBOOK-DOC1=D, D-NOTEBOOK-TRUST1=D).

mod document;
mod eval;
mod kernel;
mod protocol;
mod trust;

pub use document::{
    export_jet, export_ipynb, import_ipynb, load_jetnb, merge_by_id, save_jetnb, CellKind,
    CellOutput, JetNotebook, LossReport, MergeConflict, NotebookCell, OutputCacheEntry,
    OUTPUT_CACHE_POLICY,
};
pub use kernel::{CellExecResult, ClientKind, Kernel, KernelView, RerunDecision};
pub use protocol::{handle_message, run_headless_script, ProtocolMessage, ProtocolReply};
pub use trust::{
    decide_render, grant_active, grant_key, is_granted, quarantine_outputs, revoke_matching,
    trust_store_path, ActiveRequest, MimeBundle, RenderDecision, TrustGrant, TrustStore,
    POLICY_VERSION,
};
