//! Dependency-free Canvas browser projection assets.
#![allow(non_snake_case)]
#![deny(warnings)]

mod html;
mod js;

pub use html::{canvas_html, canvas_html_for, canvas_html_query};
pub use js::canvas_js;
