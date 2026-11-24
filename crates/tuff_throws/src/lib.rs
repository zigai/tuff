//! Exception-flow analysis for Python projects.
//!
//! [`extract`] collects per-file facts from the syntax tree, [`ty_adapter`] resolves call
//! targets and exception classes with ty, [`graph`] propagates exceptions across the call
//! graph, and [`check`] and [`inspect`] turn the results into diagnostics and reports.
//! [`snapshot`] packages an analysis for editors: per-file diagnostics and hover lookups.

pub mod check;
pub mod config;
pub mod extract;
pub mod graph;
pub mod hierarchy;
pub mod inspect;
pub mod snapshot;
pub mod ty_adapter;

use anyhow::Result;

use crate::config::LoadedConfig;
use crate::graph::Analysis;
use crate::ty_adapter::{Overrides, TyProject};

/// Analyzes the project rooted at the configuration's root.
pub fn analyze(loaded: &LoadedConfig) -> Result<Analysis> {
    analyze_with_overrides(loaded, &Overrides::default())
}

/// Analyzes the project rooted at the configuration's root, reading the files in `overrides`
/// from memory instead of from disk.
pub fn analyze_with_overrides(loaded: &LoadedConfig, overrides: &Overrides) -> Result<Analysis> {
    let project = TyProject::open(&loaded.root, overrides)?;
    Analysis::build(&project, loaded)
}
