//! Command-line interface definition.

use clap::{Parser, ValueEnum};
use std::path::PathBuf;

/// The colour theme to render with.
#[derive(Clone, Copy, Debug, Default, ValueEnum)]
pub enum Theme {
    /// Light canvas (default).
    #[default]
    Light,
    /// Dark canvas with light text.
    Dark,
}

#[derive(Parser, Debug)]
#[command(
    name = "flowghetti",
    about = "Export glowwiththeflow Terraform code to a Graphviz network-flow graph"
)]
pub struct Cli {
    /// Terraform root directory to analyse.
    pub dir: PathBuf,

    /// Graph orientation passed to Graphviz (rankdir): LR, TB, RL, BT.
    #[arg(long, default_value = "LR")]
    pub rankdir: String,

    /// Colour theme: light (default) or dark.
    #[arg(long, value_enum, default_value = "light")]
    pub theme: Theme,

    /// Include a legend documenting node categories.
    #[arg(long)]
    pub legend: bool,

    /// Add a title at the top of the graph.
    #[arg(long)]
    pub title: Option<String>,
}
