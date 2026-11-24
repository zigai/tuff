use std::collections::BTreeMap;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Arc, Condvar, LazyLock, Mutex, MutexGuard, PoisonError, Weak};

use anyhow::{Context as _, Result, anyhow, bail};
use rustc_hash::FxHashMap;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::manifest::Manifest;

/// The `tuff_rules` package, installed into each worker from the first frame.
pub(crate) const MODULES: [(&str, &str); 4] = [
    (
        "tuff_rules._api",
        include_str!("../python/tuff_rules/_api.py"),
    ),
    (
        "tuff_rules",
        include_str!("../python/tuff_rules/__init__.py"),
    ),
    (
        "tuff_rules.testing",
        include_str!("../python/tuff_rules/testing.py"),
    ),
    (
        "tuff_rules._worker",
        include_str!("../python/tuff_rules/_worker.py"),
    ),
];

/// Reads the module frame, registers every module in `sys.modules`, executes them in frame
/// order, and runs the worker loop. Only the standard library is required.
const BOOTSTRAP: &str = r#"import json, linecache, struct, sys, types
stream = sys.stdin.buffer
(size,) = struct.unpack(">I", stream.read(4))
modules = json.loads(stream.read(size).decode("utf-8"))
for name, source in sorted(modules, key=lambda module: module[0].count(".")):
    module = types.ModuleType(name)
    module.__file__ = "<tuff>/" + name.replace(".", "/") + ".py"
    parent, _, child = name.rpartition(".")
    if parent:
        setattr(sys.modules[parent], child, module)
    else:
        module.__path__ = []
    sys.modules[name] = module
for name, source in modules:
    module = sys.modules[name]
    linecache.cache[module.__file__] = (len(source), None, source.splitlines(True), module.__file__)
    exec(compile(source, module.__file__, "exec"), module.__dict__)
sys.modules["tuff_rules._worker"].main()
"#;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct WorkerSpec {
    pub(crate) python: PathBuf,
    pub(crate) root: PathBuf,
    pub(crate) plugin_dirs: Vec<PathBuf>,
    pub(crate) rule_files: Vec<PathBuf>,
}

#[derive(Serialize)]
struct ManifestRequest {
    command: &'static str,
    root: String,
    plugin_dirs: Vec<String>,
    files: Vec<String>,
}

#[derive(Serialize)]
struct CheckRequest<'a> {
    command: &'static str,
    files: [&'a FilePayload<'a>; 1],
}

#[derive(Serialize)]
pub(crate) struct FilePayload<'a> {
    pub(crate) path: String,
    pub(crate) rel_path: String,
    pub(crate) source: &'a str,
    pub(crate) qualified_names: Vec<(u32, u32, String)>,
    pub(crate) comments: Vec<(u32, u32)>,
    pub(crate) active_rules: Vec<&'static str>,
    pub(crate) options_by_rule: BTreeMap<&'static str, &'a serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct CheckResult {
    pub(crate) findings: Vec<RawFinding>,
    pub(crate) errors: Vec<RawFailure>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawFinding {
    pub(crate) rule: String,
    pub(crate) start: u64,
    pub(crate) end: u64,
    pub(crate) message: String,
    pub(crate) edits: Vec<RawEdit>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawEdit {
    pub(crate) start: u64,
    pub(crate) end: u64,
    pub(crate) replacement: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawFailure {
    pub(crate) rule: Option<String>,
    pub(crate) message: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum Reply<T> {
    Result(T),
    Error(String),
}

/// A failed exchange. `Rejected` leaves the worker usable; `Broken` does not.
enum ExchangeError {
    Rejected(String),
    Broken(anyhow::Error),
}

struct Worker {
    child: Child,
    stdin: Option<BufWriter<ChildStdin>>,
    stdout: BufReader<ChildStdout>,
}

impl Worker {
    fn start(spec: &WorkerSpec) -> Result<(Self, Manifest)> {
        log::debug!("Starting Python rule worker {}", spec.python.display());
        let mut child = Command::new(&spec.python)
            // CPython validates cached bytecode by source size and whole-second mtime, so a
            // same-size edit right after a worker imported a rule would load stale code.
            .args(["-I", "-B", "-c", BOOTSTRAP])
            .current_dir(&spec.root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .with_context(|| format!("failed to start {}", spec.python.display()))?;
        let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            bail!("worker pipes are unavailable");
        };
        let mut worker = Self {
            child,
            stdin: Some(BufWriter::new(stdin)),
            stdout: BufReader::new(stdout),
        };
        let request = ManifestRequest {
            command: "manifest",
            root: spec.root.to_string_lossy().into_owned(),
            plugin_dirs: spec
                .plugin_dirs
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect(),
            files: spec
                .rule_files
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect(),
        };
        let manifest = worker
            .send(&MODULES)
            .map_err(ExchangeError::Broken)
            .and_then(|()| worker.exchange(&request));
        match manifest {
            Ok(manifest) => Ok((worker, manifest)),
            Err(ExchangeError::Rejected(message)) => {
                Err(anyhow!("failed to load Python rules: {message}"))
            }
            Err(ExchangeError::Broken(error)) => Err(worker.broken(error)),
        }
    }

    fn send(&mut self, message: &impl Serialize) -> Result<()> {
        let payload = serde_json::to_vec(message)?;
        let length = u32::try_from(payload.len()).context("worker request is too large")?;
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| anyhow!("worker input is closed"))?;
        stdin.write_all(&length.to_be_bytes())?;
        stdin.write_all(&payload)?;
        stdin.flush()?;
        Ok(())
    }

    fn receive<T: DeserializeOwned>(&mut self) -> Result<Reply<T>> {
        let mut header = [0; 4];
        self.stdout.read_exact(&mut header)?;
        let mut payload = vec![0; u32::from_be_bytes(header) as usize];
        self.stdout.read_exact(&mut payload)?;
        Ok(serde_json::from_slice(&payload)?)
    }

    fn exchange<T: DeserializeOwned>(
        &mut self,
        request: &impl Serialize,
    ) -> std::result::Result<T, ExchangeError> {
        self.send(request).map_err(ExchangeError::Broken)?;
        match self.receive().map_err(ExchangeError::Broken)? {
            Reply::Result(value) => Ok(value),
            Reply::Error(message) => Err(ExchangeError::Rejected(message)),
        }
    }

    /// Stops a worker whose stream is unusable and describes how it ended.
    fn broken(mut self, error: anyhow::Error) -> anyhow::Error {
        self.stdin = None;
        let _ = self.child.kill();
        match self.child.wait() {
            Ok(status) if !status.success() => {
                anyhow!("Python rule worker exited unexpectedly ({status})")
            }
            _ => error.context("Python rule worker failed"),
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        // Closing stdin ends the worker loop.
        self.stdin = None;
        let _ = self.child.wait();
    }
}

#[derive(Default)]
struct PoolState {
    idle: Vec<Worker>,
    live: usize,
}

/// Lazily started workers, at most `capacity` at a time.
pub(crate) struct WorkerPool {
    spec: WorkerSpec,
    capacity: usize,
    state: Mutex<PoolState>,
    released: Condvar,
}

impl std::fmt::Debug for WorkerPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkerPool")
            .field("spec", &self.spec)
            .field("capacity", &self.capacity)
            .finish_non_exhaustive()
    }
}

/// Configurations that share plugin files and an interpreter share workers.
pub(crate) fn shared_pool(spec: WorkerSpec, key: &str, capacity: usize) -> Arc<WorkerPool> {
    type Registry = FxHashMap<(String, WorkerSpec, usize), Weak<WorkerPool>>;
    static POOLS: LazyLock<Mutex<Registry>> = LazyLock::new(Mutex::default);
    let mut pools = POOLS.lock().unwrap_or_else(PoisonError::into_inner);
    pools.retain(|_, pool| pool.strong_count() > 0);
    let key = (key.to_owned(), spec, capacity);
    if let Some(pool) = pools.get(&key).and_then(Weak::upgrade) {
        return pool;
    }
    let pool = Arc::new(WorkerPool {
        spec: key.1.clone(),
        capacity,
        state: Mutex::new(PoolState::default()),
        released: Condvar::new(),
    });
    pools.insert(key, Arc::downgrade(&pool));
    pool
}

impl WorkerPool {
    fn lock(&self) -> MutexGuard<'_, PoolState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Starts a worker to read the manifest and keeps it for later checks.
    pub(crate) fn load_manifest(&self) -> Result<Manifest> {
        self.lock().live += 1;
        match Worker::start(&self.spec) {
            Ok((worker, manifest)) => {
                self.release(worker);
                Ok(manifest)
            }
            Err(error) => {
                self.discard();
                Err(error)
            }
        }
    }

    pub(crate) fn check(&self, file: &FilePayload<'_>) -> Result<CheckResult> {
        let mut lease = self.acquire()?;
        let request = CheckRequest {
            command: "check_batch",
            files: [file],
        };
        let Some(worker) = lease.worker.as_mut() else {
            bail!("worker lease is empty");
        };
        match worker.exchange::<Vec<CheckResult>>(&request) {
            Ok(mut results) => {
                lease.release();
                results
                    .pop()
                    .ok_or_else(|| anyhow!("Python rule worker returned no result"))
            }
            Err(ExchangeError::Rejected(message)) => {
                lease.release();
                Err(anyhow!("Python rule worker rejected the file: {message}"))
            }
            Err(ExchangeError::Broken(error)) => Err(lease.break_worker(error)),
        }
    }

    fn acquire(&self) -> Result<Lease<'_>> {
        let mut state = self.lock();
        loop {
            if let Some(worker) = state.idle.pop() {
                return Ok(Lease {
                    pool: self,
                    worker: Some(worker),
                });
            }
            if state.live < self.capacity {
                state.live += 1;
                drop(state);
                return match Worker::start(&self.spec) {
                    Ok((worker, _)) => Ok(Lease {
                        pool: self,
                        worker: Some(worker),
                    }),
                    Err(error) => {
                        self.discard();
                        Err(error)
                    }
                };
            }
            state = self
                .released
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    fn release(&self, worker: Worker) {
        self.lock().idle.push(worker);
        self.released.notify_one();
    }

    fn discard(&self) {
        let mut state = self.lock();
        state.live = state.live.saturating_sub(1);
        drop(state);
        self.released.notify_one();
    }
}

/// Returns its worker to the pool, or retires it if the exchange did not complete.
struct Lease<'pool> {
    pool: &'pool WorkerPool,
    worker: Option<Worker>,
}

impl Lease<'_> {
    fn release(&mut self) {
        if let Some(worker) = self.worker.take() {
            self.pool.release(worker);
        }
    }

    fn break_worker(&mut self, error: anyhow::Error) -> anyhow::Error {
        let error = match self.worker.take() {
            Some(worker) => worker.broken(error),
            None => error,
        };
        self.pool.discard();
        error
    }
}

impl Drop for Lease<'_> {
    fn drop(&mut self) {
        if let Some(mut worker) = self.worker.take() {
            let _ = worker.child.kill();
            drop(worker);
            self.pool.discard();
        }
    }
}
