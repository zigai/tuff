//! An analyzed project prepared for editor queries: diagnostics per file and hover lookups.

use std::path::{Path, PathBuf};

use anyhow::Result;
use ruff_text_size::{TextRange, TextSize};
use rustc_hash::FxHashMap;

use crate::check::{CheckOptions, Diagnostic, check};
use crate::config::LoadedConfig;
use crate::graph::{Analysis, AnalyzedFile, CallResolution, FunctionId};
use crate::inspect::{InspectedFunction, inspect_function};
use crate::ty_adapter::Overrides;

/// The analysis of one project and the diagnostics it produces.
#[derive(Debug)]
pub struct ThrowsSnapshot {
    analysis: Analysis,
    /// Absolute file paths, as analyzed and canonicalized, to indices into [`Analysis::files`].
    file_indices: FxHashMap<PathBuf, usize>,
    /// Functions of each file in [`Analysis::files`].
    functions: Vec<Vec<FunctionId>>,
    /// Diagnostics of each file in [`Analysis::files`].
    diagnostics: Vec<Vec<Diagnostic>>,
}

/// Exception information for the function or call under the cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThrowsHover {
    /// The hovered function name or callee name.
    pub range: TextRange,
    /// The hovered function, or every project function the hovered call may invoke.
    pub targets: Vec<HoverTarget>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoverTarget {
    /// The exceptions declared by `@throws` or the docstring, when the function has a contract.
    pub contract: Option<Vec<String>>,
    /// The exceptions inferred to escape the function and where each comes from.
    pub function: InspectedFunction,
}

impl ThrowsSnapshot {
    /// Analyzes the project, reading the files in `overrides` from memory instead of from disk.
    pub fn build(loaded: &LoadedConfig, overrides: &Overrides) -> Result<Self> {
        Ok(Self::new(crate::analyze_with_overrides(loaded, overrides)?))
    }

    pub fn new(analysis: Analysis) -> Self {
        let mut file_indices = FxHashMap::default();
        for (index, file) in analysis.files.iter().enumerate() {
            if let Ok(canonical) = file.path.canonicalize() {
                file_indices.insert(canonical, index);
            }
            file_indices.insert(file.path.clone(), index);
        }

        let mut functions = vec![Vec::new(); analysis.files.len()];
        for function in analysis.function_ids() {
            functions[analysis.node(function).position].push(function);
        }

        let mut diagnostics = vec![Vec::new(); analysis.files.len()];
        for diagnostic in check(&analysis, &CheckOptions::default()) {
            if let Some(&index) = file_indices.get(&diagnostic.path) {
                diagnostics[index].push(diagnostic);
            }
        }

        Self {
            analysis,
            file_indices,
            functions,
            diagnostics,
        }
    }

    pub fn analysis(&self) -> &Analysis {
        &self.analysis
    }

    /// Returns the analyzed file at `path`, or `None` if the file is not part of the analysis.
    pub fn file(&self, path: &Path) -> Option<&AnalyzedFile> {
        Some(&self.analysis.files[self.file_index(path)?])
    }

    pub fn diagnostics_for_file(&self, path: &Path) -> &[Diagnostic] {
        self.file_index(path)
            .map_or(&[], |index| self.diagnostics[index].as_slice())
    }

    /// Describes the exceptions of the function whose name is at `offset`, or of the project
    /// functions invoked by the call whose callee name is at `offset`.
    pub fn hover_at_offset(&self, path: &Path, offset: TextSize) -> Option<ThrowsHover> {
        let functions = &self.functions[self.file_index(path)?];

        if let Some(&function) = functions.iter().find(|function| {
            self.analysis
                .facts(**function)
                .name_range
                .contains_inclusive(offset)
        }) {
            return Some(ThrowsHover {
                range: self.analysis.facts(function).name_range,
                targets: vec![self.hover_target(function)],
            });
        }

        let (function, site, range) = functions.iter().find_map(|&function| {
            self.analysis
                .facts(function)
                .calls
                .iter()
                .enumerate()
                .find(|(_, call)| call.callee_range.contains_inclusive(offset))
                .map(|(site, call)| (function, site, call.callee_range))
        })?;
        let CallResolution::Project(callees) = self.analysis.call_resolution(function, site) else {
            return None;
        };
        if callees.is_empty() {
            return None;
        }
        Some(ThrowsHover {
            range,
            targets: callees
                .iter()
                .map(|callee| self.hover_target(*callee))
                .collect(),
        })
    }

    fn file_index(&self, path: &Path) -> Option<usize> {
        self.file_indices.get(path).copied().or_else(|| {
            let canonical = path.canonicalize().ok()?;
            self.file_indices.get(&canonical).copied()
        })
    }

    fn hover_target(&self, function: FunctionId) -> HoverTarget {
        let hierarchy = &self.analysis.hierarchy;
        let contract = self.analysis.contract(function).map(|declared| {
            let mut names: Vec<String> = Vec::new();
            for exception in declared {
                let name = hierarchy.display_name(exception);
                if !names.iter().any(|existing| existing == name) {
                    names.push(name.to_string());
                }
            }
            names
        });
        HoverTarget {
            contract,
            function: inspect_function(&self.analysis, function, &self.analysis.config.track),
        }
    }
}
