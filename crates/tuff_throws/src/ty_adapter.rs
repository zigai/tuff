//! The only module that talks to ty and `ruff_db`.
//!
//! Everything else in this crate works with plain paths, text ranges, and qualified names, so
//! changes to ty's internal APIs only need to be absorbed here.

use std::any::Any;
use std::cell::RefCell;
use std::hash::{DefaultHasher, Hash, Hasher as _};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Result, anyhow};
use ruff_db::file_revision::FileRevision;
use ruff_db::files::File;
use ruff_db::parsed::{ParsedModuleRef, parsed_module};
use ruff_db::source::source_text;
use ruff_db::system::walk_directory::WalkDirectoryBuilder;
use ruff_db::system::{
    CommandExecutor, DirectoryEntry, FileType, Metadata, OsSystem, System, SystemPath,
    SystemPathBuf, SystemVirtualPath, WhichResult, WritableSystem,
};
use ruff_notebook::{Notebook, NotebookError};
use ruff_python_ast::find_node::covering_node;
use ruff_python_ast::{AnyNodeRef, PySourceType};
use ruff_text_size::{Ranged, TextRange, TextSize};
use rustc_hash::FxHashMap;
use ty_module_resolver::{ResolverFile, file_to_module};
use ty_project::{Db as _, ProjectDatabase, ProjectMetadata};
use ty_python_semantic::types::Type;
use ty_python_semantic::{
    Db as _, HasType, SemanticModel, TypeHierarchyClass, type_hierarchy_prepare,
    type_hierarchy_supertypes,
};

use crate::extract::{ModuleContext, ModuleFacts, extract};

/// In-memory file contents, such as unsaved editor buffers, that take precedence over the
/// files on disk. Keys are absolute paths.
pub type Overrides = FxHashMap<PathBuf, String>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FileId(u32);

impl FileId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone)]
pub struct ProjectFile {
    pub path: PathBuf,
    pub module: Option<String>,
    pub is_package: bool,
    file: File,
}

/// A class as seen by the type checker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassInfo {
    pub qualified_name: String,
    /// Every base class, transitively, excluding the class itself.
    pub ancestors: Vec<String>,
}

/// A definition that a call expression may invoke.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallTarget {
    /// A definition in a project file. `focus` is the range of the definition's name.
    Project { file: FileId, focus: TextRange },
    /// A definition outside the project, such as the standard library or an installed package.
    External,
}

type ClassKey = (File, TextRange);

/// A ty project database over the Python files of one project.
pub struct TyProject {
    db: ProjectDatabase,
    root: PathBuf,
    files: Vec<ProjectFile>,
    by_file: FxHashMap<File, FileId>,
    classes: RefCell<FxHashMap<ClassKey, Option<ClassInfo>>>,
}

impl TyProject {
    /// Discovers the ty project containing `root` and indexes its Python source files, reading
    /// the contents of the files in `overrides` from memory instead of from disk.
    ///
    /// Only files that exist on disk are part of the project; an override for a file that does
    /// not exist on disk is ignored.
    pub fn open(root: &Path, overrides: &Overrides) -> Result<Self> {
        let system_root = SystemPathBuf::from_path_buf(root.to_path_buf())
            .map_err(|path| anyhow!("path is not valid UTF-8: {}", path.display()))?;
        let system = OverlaySystem::new(OsSystem::new(&system_root), overrides)?;
        let mut metadata = ProjectMetadata::discover(&system_root, &system)?;
        metadata.apply_configuration_files(&system)?;
        let mut db = ProjectDatabase::fallible(metadata, system)?;
        ruff_db::disable_lru(&mut db);

        let mut files = Vec::new();
        let mut by_file = FxHashMap::default();
        let mut indexed: Vec<File> = db.project().files(&db).iter().collect();
        indexed.sort_by(|a, b| a.path(&db).as_str().cmp(b.path(&db).as_str()));
        for file in indexed {
            let Some(path) = file.path(&db).as_system_path() else {
                continue;
            };
            if PySourceType::from(path.as_std_path()) != PySourceType::Python {
                continue;
            }
            let program_file = db.program_file(file);
            let module = file_to_module(&db, program_file.resolver_file(&db))
                .map(|module| module.name(&db).to_string());
            let id = FileId(u32::try_from(files.len())?);
            by_file.insert(file, id);
            files.push(ProjectFile {
                path: path.as_std_path().to_path_buf(),
                module,
                is_package: path.file_stem() == Some("__init__"),
                file,
            });
        }

        Ok(Self {
            db,
            root: root.to_path_buf(),
            files,
            by_file,
            classes: RefCell::default(),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn files(&self) -> &[ProjectFile] {
        &self.files
    }

    pub fn file_ids(&self) -> impl Iterator<Item = FileId> + use<> {
        (0..self.files.len()).map(|index| FileId(u32::try_from(index).unwrap_or(u32::MAX)))
    }

    pub fn file(&self, id: FileId) -> &ProjectFile {
        &self.files[id.index()]
    }

    pub fn source(&self, id: FileId) -> String {
        source_text(&self.db, self.file(id).file)
            .as_str()
            .to_string()
    }

    /// Extracts exception facts from ty's parse of the file, so that recorded ranges line up
    /// with the nodes ty infers types for.
    pub fn extract(&self, id: FileId) -> ModuleFacts {
        let project_file = self.file(id);
        let parsed = self.parsed(project_file.file);
        let source = source_text(&self.db, project_file.file);
        extract(
            &parsed,
            source.as_str(),
            ModuleContext {
                name: project_file.module.as_deref(),
                is_package: project_file.is_package,
            },
        )
    }

    /// Resolves the definitions a call may invoke, given the offset of the callee's name.
    ///
    /// Uses ty's go-to-definition, so method calls resolve through inferred receiver types
    /// (`client.fetch()`, `self.save()`), imports, and aliases. Returns `None` when ty cannot
    /// resolve the callee, for example a call through an untyped parameter.
    pub fn call_targets(&self, id: FileId, offset: TextSize) -> Option<Vec<CallTarget>> {
        let program_file = self.db.program_file(self.file(id).file);
        let targets = ty_ide::goto_definition(&self.db, program_file, offset)?;
        let targets: Vec<_> = targets
            .into_iter()
            .map(|target| match self.by_file.get(&target.file()) {
                Some(file) => CallTarget::Project {
                    file: *file,
                    focus: target.focus_range(),
                },
                None => CallTarget::External,
            })
            .collect();
        (!targets.is_empty()).then_some(targets)
    }

    /// Returns the exception class of the expression at `range`.
    ///
    /// Works for class references (`except NotFound:`) and instances (`raise NotFound()`,
    /// `raise error`).
    pub fn exception_class(&self, id: FileId, range: TextRange) -> Option<ClassInfo> {
        let file = self.file(id).file;
        let parsed = self.parsed(file);
        let covering = covering_node(parsed.syntax().into(), range);
        let expr = covering
            .ancestors()
            .filter_map(AnyNodeRef::as_expr_ref)
            .find(|expr| expr.range() == range)?;
        let model = SemanticModel::new(&self.db, self.db.program_file(file));
        let ty = expr.inferred_type(&model)?;
        self.class_info_for_type(&model, ty)
    }

    /// Returns the class defined at `name_range` in a project file.
    pub fn class_at(&self, id: FileId, name_range: TextRange) -> Option<ClassInfo> {
        self.class_at_location(self.file(id).file, name_range)
    }

    fn parsed(&self, file: File) -> ParsedModuleRef {
        let program_file = self.db.program_file(file);
        parsed_module(&self.db, program_file.python_file(&self.db)).load(&self.db)
    }

    fn class_info_for_type<'db>(
        &'db self,
        model: &SemanticModel<'db>,
        ty: Type<'db>,
    ) -> Option<ClassInfo> {
        let environment = model.program_environment();
        let class = type_hierarchy_prepare(&self.db, &environment, ty)?;
        self.class_at_location(class.file.file(&self.db), class.selection_range)
    }

    fn class_at_location(&self, file: File, name_range: TextRange) -> Option<ClassInfo> {
        let key = (file, name_range);
        if let Some(info) = self.classes.borrow().get(&key) {
            return info.clone();
        }
        // Guards against cyclic base classes while this class is being resolved.
        self.classes.borrow_mut().insert(key, None);
        let info = self.compute_class_info(file, name_range);
        self.classes.borrow_mut().insert(key, info.clone());
        info
    }

    fn compute_class_info(&self, file: File, name_range: TextRange) -> Option<ClassInfo> {
        let parsed = self.parsed(file);
        let covering = covering_node(parsed.syntax().into(), name_range);
        let class_def = covering
            .ancestors()
            .filter_map(|node| match node {
                AnyNodeRef::StmtClassDef(class_def) => Some(class_def),
                _ => None,
            })
            .find(|class_def| class_def.name.range() == name_range)?;
        let model = SemanticModel::new(&self.db, self.db.program_file(file));
        let ty = class_def.inferred_type(&model)?;
        let environment = model.program_environment();
        let class = type_hierarchy_prepare(&self.db, &environment, ty)?;

        let mut ancestors = Vec::new();
        for base in type_hierarchy_supertypes(&self.db, &environment, ty) {
            let base_name = self.qualified_name(&base);
            let base_file = base.file.file(&self.db);
            if let Some(base_info) = self.class_at_location(base_file, base.selection_range) {
                push_unique(&mut ancestors, base_info.qualified_name);
                for ancestor in base_info.ancestors {
                    push_unique(&mut ancestors, ancestor);
                }
            } else {
                push_unique(&mut ancestors, base_name);
            }
        }

        Some(ClassInfo {
            qualified_name: self.qualified_name(&class),
            ancestors,
        })
    }

    fn qualified_name(&self, class: &TypeHierarchyClass<'_>) -> String {
        match module_name(&self.db, class.file) {
            Some(module) => format!("{module}.{}", class.name),
            None => class.name.to_string(),
        }
    }
}

/// The operating system's file system with some file contents replaced by in-memory text.
#[derive(Debug, Clone)]
struct OverlaySystem {
    native: OsSystem,
    overrides: Arc<FxHashMap<SystemPathBuf, String>>,
}

impl OverlaySystem {
    fn new(native: OsSystem, overrides: &Overrides) -> Result<Self> {
        let overrides = overrides
            .iter()
            .map(|(path, contents)| {
                let path = SystemPathBuf::from_path_buf(path.clone())
                    .map_err(|path| anyhow!("path is not valid UTF-8: {}", path.display()))?;
                Ok((path, contents.clone()))
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            native,
            overrides: Arc::new(overrides),
        })
    }
}

impl System for OverlaySystem {
    fn path_metadata(&self, path: &SystemPath) -> io::Result<Metadata> {
        let metadata = self.native.path_metadata(path)?;
        let Some(contents) = self.overrides.get(path) else {
            return Ok(metadata);
        };
        let mut hasher = DefaultHasher::new();
        contents.hash(&mut hasher);
        Ok(Metadata::new(
            FileRevision::from(hasher.finish()),
            metadata.permissions(),
            FileType::File,
        ))
    }

    fn canonicalize_path(&self, path: &SystemPath) -> io::Result<SystemPathBuf> {
        self.native.canonicalize_path(path)
    }

    fn is_same_file(&self, first: &SystemPath, second: &SystemPath) -> io::Result<bool> {
        self.native.is_same_file(first, second)
    }

    fn which(&self, binary_name: &str) -> WhichResult {
        self.native.which(binary_name)
    }

    fn command_executor(&self) -> Option<&dyn CommandExecutor> {
        self.native.command_executor()
    }

    fn read_to_string(&self, path: &SystemPath) -> io::Result<String> {
        match self.overrides.get(path) {
            Some(contents) => Ok(contents.clone()),
            None => self.native.read_to_string(path),
        }
    }

    fn read_to_notebook(&self, path: &SystemPath) -> std::result::Result<Notebook, NotebookError> {
        self.native.read_to_notebook(path)
    }

    fn read_virtual_path_to_string(&self, path: &SystemVirtualPath) -> io::Result<String> {
        self.native.read_virtual_path_to_string(path)
    }

    fn read_virtual_path_to_notebook(
        &self,
        path: &SystemVirtualPath,
    ) -> std::result::Result<Notebook, NotebookError> {
        self.native.read_virtual_path_to_notebook(path)
    }

    fn path_exists(&self, path: &SystemPath) -> bool {
        self.native.path_exists(path)
    }

    fn current_directory(&self) -> &SystemPath {
        self.native.current_directory()
    }

    fn user_config_directory(&self) -> Option<SystemPathBuf> {
        self.native.user_config_directory()
    }

    fn cache_dir(&self) -> Option<SystemPathBuf> {
        self.native.cache_dir()
    }

    fn read_directory<'a>(
        &'a self,
        path: &SystemPath,
    ) -> io::Result<Box<dyn Iterator<Item = io::Result<DirectoryEntry>> + 'a>> {
        self.native.read_directory(path)
    }

    fn walk_directory(&self, path: &SystemPath) -> WalkDirectoryBuilder {
        self.native.walk_directory(path)
    }

    fn env_var(&self, name: &str) -> std::result::Result<String, std::env::VarError> {
        self.native.env_var(name)
    }

    fn as_writable(&self) -> Option<&dyn WritableSystem> {
        self.native.as_writable()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn dyn_clone(&self) -> Box<dyn System> {
        Box::new(self.clone())
    }
}

fn module_name(db: &ProjectDatabase, file: ResolverFile<'_>) -> Option<String> {
    file_to_module(db, file).map(|module| module.name(db).to_string())
}

fn push_unique(names: &mut Vec<String>, name: String) {
    if !names.contains(&name) {
        names.push(name);
    }
}
