//! Native Nix evaluator internals (E4-JP9 / D-JPK-NIXENGINE1=D).
//!
//! Partial evaluator stages are test-only until JP11 opens a verified product
//! boundary. Nothing in this module shells out to an evaluator or links one.

mod Boundary;

#[cfg(test)]
mod Tests;
