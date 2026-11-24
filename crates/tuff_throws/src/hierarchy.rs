//! Exception class identities and inheritance.

use rustc_hash::{FxHashMap, FxHashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ExceptionId(u32);

/// Interned exception classes with their known ancestors.
///
/// Ancestors come from the type checker's view of each class's bases. Built-in exceptions that
/// were never seen through the type checker (for example, names that only appear in docstrings)
/// fall back to [`BUILTIN_EXCEPTIONS`].
#[derive(Debug, Default)]
pub struct ExceptionHierarchy {
    names: Vec<String>,
    by_name: FxHashMap<String, ExceptionId>,
    ancestors: Vec<Option<FxHashSet<ExceptionId>>>,
    project_defined: Vec<bool>,
}

const BASE_EXCEPTION: &str = "builtins.BaseException";
const EXCEPTION: &str = "builtins.Exception";

impl ExceptionHierarchy {
    pub fn intern(&mut self, qualified_name: &str) -> ExceptionId {
        if let Some(id) = self.by_name.get(qualified_name) {
            return *id;
        }
        let id = ExceptionId(u32::try_from(self.names.len()).unwrap_or(u32::MAX));
        self.names.push(qualified_name.to_string());
        self.by_name.insert(qualified_name.to_string(), id);
        self.ancestors.push(None);
        self.project_defined.push(false);
        if let Some(parent) = builtin_parent(qualified_name) {
            let parent = self.intern(&format!("builtins.{parent}"));
            let mut ancestors = self.ancestors[parent.index()].clone().unwrap_or_default();
            ancestors.insert(parent);
            self.ancestors[id.index()] = Some(ancestors);
        }
        id
    }

    /// Records a class's ancestors (qualified names, not including the class itself).
    pub fn set_ancestors(&mut self, id: ExceptionId, ancestors: &[String]) {
        let ancestors = ancestors.iter().map(|name| self.intern(name)).collect();
        self.ancestors[id.index()] = Some(ancestors);
    }

    pub fn mark_project_defined(&mut self, id: ExceptionId) {
        self.project_defined[id.index()] = true;
    }

    pub fn get(&self, qualified_name: &str) -> Option<ExceptionId> {
        self.by_name.get(qualified_name).copied()
    }

    pub fn name(&self, id: ExceptionId) -> &str {
        &self.names[id.index()]
    }

    /// The class name without its module, as shown in messages.
    pub fn display_name(&self, id: ExceptionId) -> &str {
        let name = self.name(id);
        name.rsplit('.').next().unwrap_or(name)
    }

    pub fn is_project_defined(&self, id: ExceptionId) -> bool {
        self.project_defined[id.index()]
    }

    pub fn has_known_ancestors(&self, id: ExceptionId) -> bool {
        self.ancestors[id.index()].is_some()
    }

    pub fn ancestors(&self, id: ExceptionId) -> impl Iterator<Item = ExceptionId> + '_ {
        self.ancestors[id.index()].iter().flatten().copied()
    }

    /// Returns whether `class` is `base` or inherits from it.
    ///
    /// A class with unknown ancestry is assumed to derive from `Exception`, which is true for
    /// nearly every exception class and keeps broad handlers effective.
    pub fn is_subclass(&self, class: ExceptionId, base: ExceptionId) -> bool {
        if class == base {
            return true;
        }
        match &self.ancestors[class.index()] {
            Some(ancestors) => ancestors.contains(&base),
            None => matches!(self.name(base), BASE_EXCEPTION | EXCEPTION),
        }
    }

    /// Returns whether an exception named `pattern` matches `class` or one of its ancestors.
    ///
    /// `pattern` is either a qualified name (`app.errors.DomainError`) or a bare class name
    /// (`DomainError`), which matches any class with that name.
    pub fn derives_from_name(&self, class: ExceptionId, pattern: &str) -> bool {
        std::iter::once(class)
            .chain(self.ancestors(class))
            .any(|id| name_matches(self.name(id), pattern))
    }

    pub fn ids(&self) -> impl Iterator<Item = ExceptionId> {
        (0..self.names.len()).map(|index| ExceptionId(u32::try_from(index).unwrap_or(u32::MAX)))
    }
}

impl ExceptionId {
    fn index(self) -> usize {
        self.0 as usize
    }
}

pub fn name_matches(qualified_name: &str, pattern: &str) -> bool {
    let pattern = pattern.strip_prefix("builtins.").unwrap_or(pattern);
    let name = qualified_name
        .strip_prefix("builtins.")
        .unwrap_or(qualified_name);
    name == pattern
        || name
            .strip_suffix(pattern)
            .is_some_and(|prefix| prefix.ends_with('.'))
}

fn builtin_parent(qualified_name: &str) -> Option<&'static str> {
    let name = qualified_name.strip_prefix("builtins.")?;
    BUILTIN_EXCEPTIONS
        .iter()
        .find(|(class, _)| *class == name)
        .and_then(|(_, parent)| *parent)
}

pub fn is_builtin_exception(name: &str) -> bool {
    BUILTIN_EXCEPTIONS.iter().any(|(class, _)| *class == name)
}

/// The built-in exception hierarchy as `(class, parent)` pairs.
const BUILTIN_EXCEPTIONS: &[(&str, Option<&str>)] = &[
    ("BaseException", None),
    ("BaseExceptionGroup", Some("BaseException")),
    ("GeneratorExit", Some("BaseException")),
    ("KeyboardInterrupt", Some("BaseException")),
    ("SystemExit", Some("BaseException")),
    ("Exception", Some("BaseException")),
    ("ArithmeticError", Some("Exception")),
    ("FloatingPointError", Some("ArithmeticError")),
    ("OverflowError", Some("ArithmeticError")),
    ("ZeroDivisionError", Some("ArithmeticError")),
    ("AssertionError", Some("Exception")),
    ("AttributeError", Some("Exception")),
    ("BufferError", Some("Exception")),
    ("EOFError", Some("Exception")),
    ("ExceptionGroup", Some("BaseExceptionGroup")),
    ("ImportError", Some("Exception")),
    ("ModuleNotFoundError", Some("ImportError")),
    ("LookupError", Some("Exception")),
    ("IndexError", Some("LookupError")),
    ("KeyError", Some("LookupError")),
    ("MemoryError", Some("Exception")),
    ("NameError", Some("Exception")),
    ("UnboundLocalError", Some("NameError")),
    ("OSError", Some("Exception")),
    ("BlockingIOError", Some("OSError")),
    ("ChildProcessError", Some("OSError")),
    ("ConnectionError", Some("OSError")),
    ("BrokenPipeError", Some("ConnectionError")),
    ("ConnectionAbortedError", Some("ConnectionError")),
    ("ConnectionRefusedError", Some("ConnectionError")),
    ("ConnectionResetError", Some("ConnectionError")),
    ("FileExistsError", Some("OSError")),
    ("FileNotFoundError", Some("OSError")),
    ("InterruptedError", Some("OSError")),
    ("IsADirectoryError", Some("OSError")),
    ("NotADirectoryError", Some("OSError")),
    ("PermissionError", Some("OSError")),
    ("ProcessLookupError", Some("OSError")),
    ("TimeoutError", Some("OSError")),
    ("ReferenceError", Some("Exception")),
    ("RuntimeError", Some("Exception")),
    ("NotImplementedError", Some("RuntimeError")),
    ("PythonFinalizationError", Some("RuntimeError")),
    ("RecursionError", Some("RuntimeError")),
    ("StopAsyncIteration", Some("Exception")),
    ("StopIteration", Some("Exception")),
    ("SyntaxError", Some("Exception")),
    ("IndentationError", Some("SyntaxError")),
    ("TabError", Some("IndentationError")),
    ("SystemError", Some("Exception")),
    ("TypeError", Some("Exception")),
    ("ValueError", Some("Exception")),
    ("UnicodeError", Some("ValueError")),
    ("UnicodeDecodeError", Some("UnicodeError")),
    ("UnicodeEncodeError", Some("UnicodeError")),
    ("UnicodeTranslateError", Some("UnicodeError")),
    ("Warning", Some("Exception")),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_fallback_hierarchy() {
        let mut hierarchy = ExceptionHierarchy::default();
        let key_error = hierarchy.intern("builtins.KeyError");
        let lookup = hierarchy.intern("builtins.LookupError");
        let value_error = hierarchy.intern("builtins.ValueError");
        let exception = hierarchy.intern("builtins.Exception");
        assert!(hierarchy.is_subclass(key_error, lookup));
        assert!(hierarchy.is_subclass(key_error, exception));
        assert!(!hierarchy.is_subclass(key_error, value_error));
        assert!(!hierarchy.is_subclass(lookup, key_error));
    }

    #[test]
    fn recorded_ancestors() {
        let mut hierarchy = ExceptionHierarchy::default();
        let not_found = hierarchy.intern("app.errors.NotFound");
        hierarchy.set_ancestors(
            not_found,
            &[
                "app.errors.DomainError".to_string(),
                "builtins.Exception".to_string(),
                "builtins.BaseException".to_string(),
            ],
        );
        let domain = hierarchy.intern("app.errors.DomainError");
        let value_error = hierarchy.intern("builtins.ValueError");
        assert!(hierarchy.is_subclass(not_found, domain));
        assert!(!hierarchy.is_subclass(not_found, value_error));
        assert!(hierarchy.derives_from_name(not_found, "DomainError"));
        assert!(hierarchy.derives_from_name(not_found, "app.errors.DomainError"));
        assert!(!hierarchy.derives_from_name(not_found, "errors.Domain"));
    }

    #[test]
    fn unknown_ancestry_derives_from_exception_only() {
        let mut hierarchy = ExceptionHierarchy::default();
        let unknown = hierarchy.intern("vendor.Error");
        let exception = hierarchy.intern("builtins.Exception");
        let value_error = hierarchy.intern("builtins.ValueError");
        assert!(hierarchy.is_subclass(unknown, exception));
        assert!(!hierarchy.is_subclass(unknown, value_error));
    }

    #[test]
    fn name_patterns() {
        assert!(name_matches("builtins.ValueError", "ValueError"));
        assert!(name_matches("app.errors.NotFound", "errors.NotFound"));
        assert!(!name_matches("app.errors.NotFound", "Found"));
    }
}
