// Proof-only Rust-tier reference: call the canonical writer from the freshly
// composed all-parts rlib, never a copied writer or a historical pack.
extern crate jet_runtime;
fn main() {
    use jet_runtime::jet_mem::{jet_memory_ledger_record, MemoryLedgerWitness};
    jet_memory_ledger_record(MemoryLedgerWitness {
        kind: "sentry",
        code: "R0802",
        source: "stack.jet",
        span_start: 1,
        span_end: 1,
        byte_spans: false,
        scope: "same-scope",
        provenance: "source #Unsafe gate",
        detail: "the owning Jet frame has expired",
        expected: None,
        repairs: &[
            "move the raw access before the storage is released",
            "replace the raw pointer with an owned value",
        ],
    }).expect("record canonical Rust ledger witness");
}
