//! flowghetti — static analysis of Terraform code consuming the
//! `glowwiththeflow` module, rendered as a Graphviz network-flow graph.
//!
//! The pipeline has four decoupled stages:
//!
//! ```text
//! parse (HCL) -> resolve (refs) -> build (domain graph) -> render (DOT)
//! ```

pub mod build;
pub mod cli;
pub mod error;
pub mod model;
pub mod parse;
pub mod render;
pub mod resolve;

pub use error::{Error, Result};

use render::ThemeChoice;
use std::path::Path;

/// Run the full pipeline on a Terraform root directory, returning DOT text.
pub fn run(
    dir: &Path,
    rankdir: &str,
    theme: ThemeChoice,
    legend: bool,
    title: Option<&str>,
) -> Result<String> {
    let config = parse::load(dir)?;
    let (resources, flows, directives) = resolve::resolve(&config);
    let graph = build::build(&resources, &flows, &directives);
    Ok(render::to_dot(&graph, rankdir, theme, legend, title))
}
