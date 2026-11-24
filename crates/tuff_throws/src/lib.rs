//! Exception-flow analysis for Python projects.
//!
//! [`extract`] collects per-file facts from the syntax tree, [`ty_adapter`] resolves call
//! targets and exception classes with ty, [`graph`] propagates exceptions across the call
//! graph, and [`check`] and [`inspect`] turn the results into diagnostics and reports.

pub mod check;
pub mod config;
pub mod extract;
pub mod graph;
pub mod hierarchy;
pub mod inspect;
pub mod ty_adapter;

use anyhow::Result;

use crate::config::LoadedConfig;
use crate::graph::Analysis;
use crate::ty_adapter::TyProject;

/// Analyzes the project rooted at the configuration's root.
pub fn analyze(loaded: &LoadedConfig) -> Result<Analysis> {
    let project = TyProject::open(&loaded.root)?;
    Analysis::build(&project, loaded)
}
