//! The Rust support crate of the `core.plugin` host bridge
//! (RustBridge/Runtime/Plugin.rs): the authority holds relation, the plugin
//! wire codec and rooted no-follow file access. Everything else in Jet's
//! foundation is Jet (Compiler/JetFoundation, Jetpack/Foundation).
#![allow(non_snake_case)]
pub mod Authority;
pub mod PluginWire;
pub mod RootedFiles;
