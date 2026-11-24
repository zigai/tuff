use std::path::Path;

use indoc::indoc;
use tempfile::TempDir;
use tuff_throws::check::{CheckOptions, Diagnostic, check, render_text as render_check};
use tuff_throws::config::discover;
use tuff_throws::graph::Analysis;
use tuff_throws::inspect::{inspect, render_json, render_text};
use tuff_throws::snapshot::ThrowsSnapshot;
use tuff_throws::ty_adapter::Overrides;

const ERRORS: &str = indoc! {"
    class DomainError(Exception):
        pass


    class NotFound(DomainError):
        pass


    class Invalid(DomainError):
        pass


    class Unrelated(Exception):
        pass
"};

const CONTRACTS: &str = indoc! {"
    def throws(*exceptions):
        def decorate(function):
            return function
        return decorate
"};

struct Project {
    dir: TempDir,
}

impl Project {
    fn new(config: &str, files: &[(&str, &str)]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("pyproject.toml"),
            format!("[project]\nname = \"sample\"\nversion = \"0.1.0\"\nrequires-python = \">=3.12\"\n\n[tool.tuff.throws]\n{config}"),
        )
        .unwrap();
        write(root, "app/__init__.py", "");
        write(root, "app/errors.py", ERRORS);
        write(root, "app/contracts.py", CONTRACTS);
        for (path, contents) in files {
            write(root, path, contents);
        }
        Self { dir }
    }

    fn analyze(&self) -> Analysis {
        let loaded = discover(self.dir.path()).unwrap();
        tuff_throws::analyze(&loaded).unwrap()
    }
}

fn write(root: &Path, path: &str, contents: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

/// Qualified names of the exceptions escaping `target`.
fn escaping(analysis: &Analysis, target: &str) -> Vec<String> {
    let functions = analysis.find(target);
    assert_eq!(
        functions.len(),
        1,
        "expected exactly one match for {target}"
    );
    let mut names: Vec<String> = analysis
        .result(functions[0])
        .exceptions
        .keys()
        .map(|id| {
            let name = analysis.hierarchy.name(*id);
            name.strip_prefix("builtins.").unwrap_or(name).to_string()
        })
        .collect();
    names.sort();
    names
}

fn codes(diagnostics: &[Diagnostic]) -> Vec<(String, &'static str, String)> {
    diagnostics
        .iter()
        .map(|diagnostic| {
            (
                diagnostic.location.to_string(),
                diagnostic.code.code(),
                diagnostic.exception.clone(),
            )
        })
        .collect()
}

#[test]
fn raises_reraises_and_handlers() {
    let project = Project::new(
        "",
        &[(
            "app/core.py",
            indoc! {"
                import contextlib

                from app.errors import DomainError, Invalid, NotFound


                def direct():
                    raise NotFound('missing')


                def reraise():
                    try:
                        direct()
                    except DomainError:
                        print('failed')
                        raise


                def reraise_bound_name():
                    try:
                        direct()
                    except NotFound as err:
                        raise err


                def chained():
                    try:
                        direct()
                    except NotFound as err:
                        raise Invalid() from err


                def caught_by_base():
                    try:
                        direct()
                    except DomainError:
                        pass


                def parse():
                    raise ValueError('bad')


                def caught_by_exception():
                    try:
                        parse()
                    except Exception:
                        return None


                def subclass_handler_does_not_catch_base():
                    try:
                        raise DomainError()
                    except NotFound:
                        pass


                def suppressed():
                    with contextlib.suppress(NotFound):
                        direct()


                def suppressed_other():
                    with contextlib.suppress(KeyError):
                        direct()


                def handler_body_is_not_guarded():
                    try:
                        direct()
                    except NotFound:
                        parse()


                def bare_except():
                    try:
                        direct()
                        parse()
                    except:
                        pass


                def raise_variable():
                    error = Invalid()
                    raise error
            "},
        )],
    );
    let analysis = project.analyze();
    let module = "app/core.py";
    let escapes = |name: &str| escaping(&analysis, &format!("{module}::{name}"));

    assert_eq!(escapes("direct"), ["app.errors.NotFound"]);
    assert_eq!(escapes("reraise"), ["app.errors.NotFound"]);
    assert_eq!(escapes("reraise_bound_name"), ["app.errors.NotFound"]);
    assert_eq!(escapes("chained"), ["app.errors.Invalid"]);
    assert!(escapes("caught_by_base").is_empty());
    assert_eq!(escapes("parse"), ["ValueError"]);
    assert!(escapes("caught_by_exception").is_empty());
    assert_eq!(
        escapes("subclass_handler_does_not_catch_base"),
        ["app.errors.DomainError"]
    );
    assert!(escapes("suppressed").is_empty());
    assert_eq!(escapes("suppressed_other"), ["app.errors.NotFound"]);
    assert_eq!(escapes("handler_body_is_not_guarded"), ["ValueError"]);
    assert!(escapes("bare_except").is_empty());
    assert_eq!(escapes("raise_variable"), ["app.errors.Invalid"]);
}

#[test]
fn propagation_across_modules_and_methods() {
    let project = Project::new(
        "",
        &[
            (
                "app/client.py",
                indoc! {"
                    from app.errors import NotFound


                    class Client:
                        def fetch(self, key):
                            raise NotFound(key)


                    class CachingClient(Client):
                        pass
                "},
            ),
            (
                "app/service.py",
                indoc! {"
                    from app import client as client_module
                    from app.client import CachingClient, Client
                    from app.errors import Invalid


                    class Service:
                        def __init__(self, client: Client):
                            self.client = client

                        def run(self):
                            return self.client.fetch('key')

                        def save(self):
                            self.run()


                    class Validated:
                        def __init__(self, value):
                            if not value:
                                raise Invalid()


                    def inherited_method(client: CachingClient):
                        client.fetch('key')


                    def module_attribute():
                        client_module.Client().fetch('key')


                    def constructor():
                        Validated(None)


                    def local_instance():
                        client = Client()
                        client.fetch('key')
                "},
            ),
        ],
    );
    let analysis = project.analyze();
    let escapes = |name: &str| escaping(&analysis, &format!("app/service.py::{name}"));

    assert_eq!(escapes("Service.run"), ["app.errors.NotFound"]);
    assert_eq!(escapes("Service.save"), ["app.errors.NotFound"]);
    assert_eq!(escapes("inherited_method"), ["app.errors.NotFound"]);
    assert_eq!(escapes("module_attribute"), ["app.errors.NotFound"]);
    assert_eq!(escapes("constructor"), ["app.errors.Invalid"]);
    assert_eq!(escapes("local_instance"), ["app.errors.NotFound"]);

    let save = analysis.find("app.service.Service.save");
    let steps: Vec<_> = analysis
        .chain(
            save[0],
            analysis.hierarchy.get("app.errors.NotFound").unwrap(),
        )
        .iter()
        .map(|step| analysis.facts(step.function).qualname.clone())
        .collect();
    assert_eq!(steps, ["Service.save", "Service.run", "Client.fetch"]);
}

#[test]
fn mutual_recursion_converges() {
    let project = Project::new(
        "",
        &[(
            "app/walk.py",
            indoc! {"
                from app.errors import Invalid, NotFound


                def even(n):
                    if n < 0:
                        raise NotFound()
                    return n == 0 or odd(n - 1)


                def odd(n):
                    if n > 100:
                        raise Invalid()
                    return n != 0 and even(n - 1)


                def countdown(n):
                    if n == 0:
                        raise Invalid()
                    countdown(n - 1)


                def caller():
                    return even(3)
            "},
        )],
    );
    let analysis = project.analyze();
    let both = ["app.errors.Invalid", "app.errors.NotFound"];
    assert_eq!(escaping(&analysis, "app/walk.py::even"), both);
    assert_eq!(escaping(&analysis, "app/walk.py::odd"), both);
    assert_eq!(escaping(&analysis, "app/walk.py::caller"), both);
    assert_eq!(
        escaping(&analysis, "app/walk.py::countdown"),
        ["app.errors.Invalid"]
    );

    // Each exception's chain ends at its raise rather than looping through the cycle.
    let caller = analysis.find("app/walk.py::caller")[0];
    let invalid = analysis.hierarchy.get("app.errors.Invalid").unwrap();
    let steps: Vec<_> = analysis
        .chain(caller, invalid)
        .iter()
        .map(|step| analysis.facts(step.function).qualname.clone())
        .collect();
    assert_eq!(steps, ["caller", "even", "odd"]);
}

#[test]
fn max_depth_limits_propagation() {
    let project = Project::new(
        "max-depth = 1\n",
        &[(
            "app/deep.py",
            indoc! {"
                from app.errors import NotFound


                def leaf():
                    raise NotFound()


                def middle():
                    leaf()


                def top():
                    middle()
            "},
        )],
    );
    let analysis = project.analyze();
    assert_eq!(
        escaping(&analysis, "app/deep.py::middle"),
        ["app.errors.NotFound"]
    );
    assert!(escaping(&analysis, "app/deep.py::top").is_empty());
    let top = analysis.find("app/deep.py::top")[0];
    assert!(analysis.result(top).incomplete);
}

#[test]
fn async_gather_and_task_groups() {
    let project = Project::new(
        "",
        &[(
            "app/tasks.py",
            indoc! {"
                import asyncio

                from app.errors import Invalid, NotFound


                async def load():
                    raise NotFound()


                async def store():
                    raise Invalid()


                async def gathered():
                    await asyncio.gather(load(), store())


                async def collected():
                    await asyncio.gather(load(), store(), return_exceptions=True)


                async def grouped():
                    try:
                        async with asyncio.TaskGroup() as group:
                            group.create_task(load())
                            group.create_task(store())
                    except* NotFound:
                        pass
            "},
        )],
    );
    let analysis = project.analyze();
    assert_eq!(
        escaping(&analysis, "app/tasks.py::gathered"),
        ["app.errors.Invalid", "app.errors.NotFound"]
    );
    assert!(escaping(&analysis, "app/tasks.py::collected").is_empty());
    assert_eq!(
        escaping(&analysis, "app/tasks.py::grouped"),
        ["app.errors.Invalid"]
    );
}

#[test]
fn contracts_propagate_and_sources_filter() {
    let files = [(
        "app/contracts_use.py",
        indoc! {r#"
            from app.contracts import throws
            from app.errors import Invalid, NotFound


            @throws(NotFound, "when the record is missing")
            def declared():
                pass


            def documented():
                """Load a record.

                Raises:
                    Invalid: If the record is malformed.
                """


            def caller():
                declared()
                documented()
        "#},
    )];

    let analysis = Project::new("", &files).analyze();
    assert_eq!(
        escaping(&analysis, "app/contracts_use.py::caller"),
        ["app.errors.Invalid", "app.errors.NotFound"]
    );

    let analysis = Project::new("sources = [\"raise\", \"decorator\"]\n", &files).analyze();
    assert_eq!(
        escaping(&analysis, "app/contracts_use.py::caller"),
        ["app.errors.NotFound"]
    );
}

#[test]
fn boundary_diagnostics_with_tracking_and_suppression() {
    let files = [
        (
            "app/repo.py",
            indoc! {"
                from app.errors import NotFound


                def load(key):
                    if not key:
                        raise ValueError(key)
                    raise NotFound(key)
            "},
        ),
        (
            "app/api.py",
            indoc! {"
                from app.errors import DomainError, NotFound, Unrelated
                from app.repo import load


                def get_item(key):
                    return load(key)


                def get_item_handled(key):
                    try:
                        return load(key)
                    except DomainError:
                        return None


                def get_item_ignored(key):
                    return load(key)  # tuff: ignore[THROWS001]


                def get_unrelated():
                    raise Unrelated()


                def helper():
                    return load('x')
            "},
        ),
    ];

    let analysis = Project::new("boundaries = [\"app.api.get_*\"]\n", &files).analyze();
    let diagnostics = check(&analysis, &CheckOptions::default());
    // Without `track`, every project-defined exception is tracked, but built-ins are not.
    assert_eq!(
        codes(&diagnostics),
        [
            (
                "app/api.py:6:12".to_string(),
                "THROWS001",
                "app.errors.NotFound".to_string()
            ),
            (
                "app/api.py:21:5".to_string(),
                "THROWS001",
                "app.errors.Unrelated".to_string()
            ),
        ]
    );
    insta::assert_snapshot!(render_check(&diagnostics[..1]));

    let analysis = Project::new(
        "boundaries = [\"app.api.get_*\"]\ntrack = [\"DomainError\"]\n",
        &files,
    )
    .analyze();
    let diagnostics = check(&analysis, &CheckOptions::default());
    assert_eq!(
        codes(&diagnostics),
        [(
            "app/api.py:6:12".to_string(),
            "THROWS001",
            "app.errors.NotFound".to_string()
        )]
    );

    let tracked = CheckOptions {
        track: vec!["ValueError".to_string()],
        ..CheckOptions::default()
    };
    // `except DomainError` does not handle `ValueError`.
    assert_eq!(
        codes(&check(&analysis, &tracked)),
        [
            (
                "app/api.py:6:12".to_string(),
                "THROWS001",
                "builtins.ValueError".to_string()
            ),
            (
                "app/api.py:11:16".to_string(),
                "THROWS001",
                "builtins.ValueError".to_string()
            ),
        ]
    );

    let elsewhere = CheckOptions {
        paths: vec!["app/repo.py".to_string()],
        ..CheckOptions::default()
    };
    assert!(check(&analysis, &elsewhere).is_empty());
}

#[test]
fn boundary_file_globs() {
    let project = Project::new(
        "boundaries = [\"app/handlers/**\"]\n",
        &[(
            "app/handlers/orders.py",
            indoc! {"
                from app.errors import Invalid


                def create():
                    raise Invalid()
            "},
        )],
    );
    let diagnostics = check(&project.analyze(), &CheckOptions::default());
    assert_eq!(
        codes(&diagnostics),
        [(
            "app/handlers/orders.py:5:5".to_string(),
            "THROWS001",
            "app.errors.Invalid".to_string()
        )]
    );
}

#[test]
fn redundant_except_requires_complete_resolution() {
    let project = Project::new(
        "",
        &[(
            "app/redundant.py",
            indoc! {"
                from app.errors import DomainError, Invalid, NotFound


                def pure(value):
                    return value + 1


                def raises_invalid():
                    raise Invalid()


                def redundant():
                    try:
                        pure(1)
                    except NotFound:
                        pass


                def needed():
                    try:
                        raises_invalid()
                    except (NotFound, Invalid):
                        pass


                def base_class_is_needed():
                    try:
                        raises_invalid()
                    except DomainError:
                        pass


                def callback(function):
                    try:
                        function()
                    except NotFound:
                        pass


                def builtin_handlers_are_not_checked():
                    try:
                        pure(1)
                    except KeyError:
                        pass


                def passes_project_callback(items):
                    try:
                        return list(map(raises_invalid, items))
                    except Invalid:
                        return []


                def suppressed():
                    try:
                        pure(1)
                    except NotFound:  # tuff: ignore[redundant-except]
                        pass
            "},
        )],
    );
    let diagnostics = check(&project.analyze(), &CheckOptions::default());
    assert_eq!(
        codes(&diagnostics),
        [
            (
                "app/redundant.py:15:12".to_string(),
                "THROWS002",
                "app.errors.NotFound".to_string()
            ),
            (
                "app/redundant.py:22:13".to_string(),
                "THROWS002",
                "app.errors.NotFound".to_string()
            ),
        ]
    );
}

#[test]
fn undeclared_exceptions() {
    let project = Project::new(
        "",
        &[(
            "app/declared.py",
            indoc! {r#"
                from app.contracts import throws
                from app.errors import DomainError, Invalid, NotFound


                def load():
                    raise NotFound()


                @throws(Invalid)
                def wrong_contract():
                    load()


                @throws(DomainError)
                def base_contract():
                    load()


                def documented():
                    """Load.

                    Raises:
                        Invalid: When invalid.
                    """
                    load()


                def no_contract():
                    load()


                @throws(Invalid)
                def ignored():
                    load()  # throws: ignore[NotFound]
            "#},
        )],
    );
    let diagnostics = check(&project.analyze(), &CheckOptions::default());
    assert_eq!(
        codes(&diagnostics),
        [
            (
                "app/declared.py:11:5".to_string(),
                "THROWS003",
                "app.errors.NotFound".to_string()
            ),
            (
                "app/declared.py:25:5".to_string(),
                "THROWS003",
                "app.errors.NotFound".to_string()
            ),
        ]
    );
}

#[test]
fn override_widens_parent_contract() {
    let project = Project::new(
        "",
        &[(
            "app/repos.py",
            indoc! {r#"
                from app.contracts import throws
                from app.errors import Invalid, NotFound


                class Repository:
                    @throws(NotFound)
                    def get(self, key):
                        raise NotImplementedError

                    def put(self, key):
                        pass


                class SqlRepository(Repository):
                    def get(self, key):
                        if not key:
                            raise Invalid()
                        raise NotFound()

                    def put(self, key):
                        raise Invalid()


                class CachedRepository(SqlRepository):
                    @throws(NotFound)
                    def get(self, key):
                        raise NotFound()
            "#},
        )],
    );
    let diagnostics = check(&project.analyze(), &CheckOptions::default());
    // `put` has no parent contract, and `CachedRepository.get` stays within its parent's.
    assert_eq!(
        codes(&diagnostics),
        [(
            "app/repos.py:15:9".to_string(),
            "THROWS004",
            "app.errors.Invalid".to_string()
        )]
    );
}

#[test]
fn inspect_output() {
    let project = Project::new(
        "",
        &[
            (
                "app/payments.py",
                indoc! {"
                    from app.errors import Invalid


                    def charge(amount):
                        if amount <= 0:
                            raise Invalid('amount')
                        if amount > 10_000:
                            raise OverflowError(amount)
                "},
            ),
            (
                "app/orders.py",
                indoc! {"
                    from app.errors import NotFound
                    from app.payments import charge


                    class OrderService:
                        def run(self, order_id, amount):
                            if order_id is None:
                                raise NotFound(order_id)
                            self.pay(amount)

                        def pay(self, amount):
                            charge(amount)
                "},
            ),
        ],
    );
    let analysis = project.analyze();

    let report = inspect(&analysis, "app/orders.py::OrderService.run", &[]).unwrap();
    insta::assert_snapshot!("inspect_text", render_text(&report));
    insta::assert_snapshot!("inspect_json", render_json(&report).unwrap());

    let tracked = inspect(
        &analysis,
        "app/orders.py::OrderService.run",
        &["DomainError".to_string()],
    )
    .unwrap();
    let names: Vec<_> = tracked[0]
        .exceptions
        .iter()
        .map(|exception| exception.name.as_str())
        .collect();
    assert_eq!(names, ["Invalid", "NotFound"]);

    let by_module = inspect(&analysis, "app.orders.OrderService.pay", &[]).unwrap();
    assert_eq!(by_module[0].function, "app/orders.py::OrderService.pay");

    let whole_file = inspect(&analysis, "app/orders.py", &[]).unwrap();
    assert_eq!(whole_file.len(), 2);

    assert!(inspect(&analysis, "app/orders.py::missing", &[]).is_err());
}

#[test]
fn excluded_files_do_not_shift_function_locations() {
    let project = Project::new(
        "exclude = [\"app/contracts.py\"]\n",
        &[(
            "app/core.py",
            indoc! {"
                from app.errors import NotFound


                def load():
                    raise NotFound()
            "},
        )],
    );
    let analysis = project.analyze();

    let report = inspect(&analysis, "app/core.py::load", &[]).unwrap();
    assert_eq!(report[0].location.to_string(), "app/core.py:4:5");
    assert_eq!(
        escaping(&analysis, "app/core.py::load"),
        ["app.errors.NotFound"]
    );
}

const LOAD_NOT_FOUND: &str = indoc! {"
    from app.errors import NotFound


    def load():
        raise NotFound()
"};

#[test]
fn overrides_replace_disk_contents() {
    let project = Project::new(
        "boundaries = [\"app.api.*\"]\n",
        &[
            ("app/core.py", LOAD_NOT_FOUND),
            (
                "app/api.py",
                indoc! {"
                    from app.core import load


                    def handler():
                        load()
                "},
            ),
        ],
    );
    let root = project.dir.path();
    let core = root.join("app/core.py");
    let api = root.join("app/api.py");
    let edited = indoc! {"
        from app.errors import Invalid


        def load():
            value = 1
            raise Invalid(value)
    "};
    let mut overrides = Overrides::default();
    overrides.insert(core.clone(), edited.to_string());
    // A buffer for a file that does not exist on disk is not part of the project.
    overrides.insert(
        root.join("app/unsaved.py"),
        "def f():\n    raise ValueError()\n".to_string(),
    );

    let snapshot = ThrowsSnapshot::build(&discover(root).unwrap(), &overrides).unwrap();

    assert_eq!(std::fs::read_to_string(&core).unwrap(), LOAD_NOT_FOUND);
    assert_eq!(
        escaping(snapshot.analysis(), "app/api.py::handler"),
        ["app.errors.Invalid"]
    );
    assert!(snapshot.file(&root.join("app/unsaved.py")).is_none());

    let diagnostics = snapshot.diagnostics_for_file(&api);
    assert_eq!(
        codes(diagnostics),
        [(
            "app/api.py:5:5".to_string(),
            "THROWS001",
            "app.errors.Invalid".to_string()
        )]
    );
    let api_source = snapshot.file(&api).unwrap().source();
    assert_eq!(&api_source[diagnostics[0].range], "load()");

    let origin = diagnostics[0].chain.last().unwrap();
    assert_eq!(origin.path, core);
    let core_source = snapshot.file(&core).unwrap().source();
    assert_eq!(&core_source[origin.range], "raise Invalid(value)");

    assert!(snapshot.diagnostics_for_file(&core).is_empty());
}

#[test]
fn hover_on_definitions_and_call_sites() {
    let api_source = indoc! {"
        from app.contracts import throws
        from app.core import load
        from app.errors import NotFound


        @throws(NotFound)
        def handler():
            value = 1
            return load()
    "};
    let project = Project::new(
        "",
        &[("app/core.py", LOAD_NOT_FOUND), ("app/api.py", api_source)],
    );
    let root = project.dir.path();
    let api = root.join("app/api.py");
    let snapshot = ThrowsSnapshot::build(&discover(root).unwrap(), &Overrides::default()).unwrap();
    let offset = |needle: &str| {
        let start = api_source.find(needle).unwrap() + 1;
        ruff_text_size::TextSize::try_from(start).unwrap()
    };

    let definition = snapshot.hover_at_offset(&api, offset("handler")).unwrap();
    assert_eq!(&api_source[definition.range], "handler");
    let [target] = definition.targets.as_slice() else {
        panic!("expected one hover target");
    };
    assert_eq!(target.function.function, "app/api.py::handler");
    assert_eq!(
        target.contract.as_deref(),
        Some(["NotFound".to_string()].as_slice())
    );
    let [exception] = target.function.exceptions.as_slice() else {
        panic!("expected one exception");
    };
    assert_eq!(exception.qualified_name, "app.errors.NotFound");
    let steps: Vec<_> = exception
        .chain
        .iter()
        .map(|step| {
            (
                step.function.as_str(),
                step.location.to_string(),
                step.action.as_str(),
            )
        })
        .collect();
    assert_eq!(
        steps,
        [
            ("handler", "app/api.py:9:12".to_string(), "calls `load()`"),
            ("load", "app/core.py:5:5".to_string(), "raises `NotFound`"),
        ]
    );

    let call = snapshot.hover_at_offset(&api, offset("load()")).unwrap();
    assert_eq!(&api_source[call.range], "load");
    let [target] = call.targets.as_slice() else {
        panic!("expected one hover target");
    };
    assert_eq!(target.function.function, "app/core.py::load");
    assert_eq!(target.contract, None);
    assert_eq!(target.function.exceptions[0].name, "NotFound");

    assert!(snapshot.hover_at_offset(&api, offset("value")).is_none());
    assert!(
        snapshot
            .hover_at_offset(&root.join("app/missing.py"), offset("handler"))
            .is_none()
    );
}
