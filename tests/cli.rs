mod common;
include!("cli_parts/support.rs");
#[path = "cli_parts/core.rs"]
mod cli_core;
#[path = "cli_parts/inspect.rs"]
mod cli_inspect;
