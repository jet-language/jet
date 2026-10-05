//! `core.term` stdout/stderr/stdin + terminal hosts (#1219). TTY writes use the
//! real terminal; non-TTY writes use the resident capture buffers so
//! ProgramOutput matches AOT under the process harness.
//! parity: include path=crates/jet-codegen/src/Prelude/CoreLib/Top/IoLineStream.rs

// This module includes shared Prelude source that several hosts compile,
// each using a different subset, so dead-code reports here are about the
// other hosts' usage, not about this one. Scoped to the module, never the crate.

pub(crate) mod term_prelude {
    include!("../../jet-codegen/src/Prelude/Term.rs");
}

mod human_output_semantics {
    include!("../../jet-codegen/src/Prelude/Core/HumanOutput.rs");
}

// #1480: literal Prelude source for line/byte stdin primitives. The extern "C"
// wrappers below only marshal jit heap i64 handles to/from Rust values and
// call these included functions — no logic is re-encoded here (I9). The
// nested `jet_std` mirrors only the IOError shape these functions construct
// via `.other(...)`; it carries no behavior of its own.
mod io_line_stream {
    

    

    mod jet_std {
        #[derive(Debug)]
        pub enum IOOperation {
            Read,
            Write,
            Flush,
        }
        #[derive(Debug)]
        pub struct IOContext {
            pub operation: IOOperation,
            pub resource: Option<String>,
            pub os_code: Option<i64>,
            pub cause: Option<String>,
        }
        #[derive(Debug)]
        pub enum IOError {
            Other(IOContext),
        }
        impl IOError {
            pub fn other(
                operation: IOOperation,
                resource: Option<String>,
                cause: impl ToString,
            ) -> Self {
                Self::Other(IOContext {
                    operation,
                    resource,
                    os_code: None,
                    cause: Some(cause.to_string()),
                })
            }
        }
    }

    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;

}

