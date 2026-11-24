use std::io::{Write, stdout};
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use tuff_throws::check::{self, CheckOptions};
use tuff_throws::config::discover;
use tuff_throws::inspect;

use crate::ExitStatus;
use crate::args::{HelpFormat, ThrowsCheckCommand, ThrowsCommand, ThrowsInspectCommand};

pub(crate) fn throws(command: ThrowsCommand) -> Result<ExitStatus> {
    // ty reports project discovery at info level, which would drown out the report.
    if log::max_level() == log::LevelFilter::Info {
        log::set_max_level(log::LevelFilter::Warn);
    }
    let cwd = std::env::current_dir().context("failed to read the working directory")?;
    match command {
        ThrowsCommand::Inspect(args) => inspect_command(&cwd, &args),
        ThrowsCommand::Check(args) => check_command(&cwd, args),
    }
}

fn inspect_command(cwd: &Path, args: &ThrowsInspectCommand) -> Result<ExitStatus> {
    let mut loaded = discover(cwd)?;
    if let Some(depth) = args.depth {
        loaded.config.max_depth = Some(depth);
    }
    let analysis = tuff_throws::analyze(&loaded)?;
    let report = inspect::inspect(&analysis, &args.target, &args.track)?;
    let output = match args.format {
        HelpFormat::Text => inspect::render_text(&report),
        HelpFormat::Json => inspect::render_json(&report)? + "\n",
    };
    stdout().lock().write_all(output.as_bytes())?;
    Ok(ExitStatus::Success)
}

fn check_command(cwd: &Path, args: ThrowsCheckCommand) -> Result<ExitStatus> {
    let mut loaded = discover(cwd)?;
    loaded.config.boundaries.extend(args.boundary);
    let analysis = tuff_throws::analyze(&loaded)?;

    let mut paths = args
        .paths
        .iter()
        .map(|path| relative_path(&analysis.root, &cwd.join(path)))
        .collect::<Result<Vec<_>>>()?;
    if args.diff {
        let changed = changed_files(&analysis.root)?;
        let selected: Vec<String> = changed
            .into_iter()
            .filter(|file| {
                paths.is_empty()
                    || paths.iter().any(|path| {
                        path == "." || file == path || file.starts_with(&format!("{path}/"))
                    })
            })
            .collect();
        if selected.is_empty() {
            return finish(&[], args.format);
        }
        paths = selected;
    }

    let diagnostics = check::check(
        &analysis,
        &CheckOptions {
            paths,
            track: args.track,
        },
    );
    finish(&diagnostics, args.format)
}

fn finish(diagnostics: &[check::Diagnostic], format: HelpFormat) -> Result<ExitStatus> {
    let output = match format {
        HelpFormat::Text => check::render_text(diagnostics),
        HelpFormat::Json => check::render_json(diagnostics)? + "\n",
    };
    stdout().lock().write_all(output.as_bytes())?;
    Ok(if diagnostics.is_empty() {
        ExitStatus::Success
    } else {
        ExitStatus::Failure
    })
}

fn relative_path(root: &Path, path: &Path) -> Result<String> {
    let path = path
        .canonicalize()
        .with_context(|| format!("failed to find {}", path.display()))?;
    let relative = path.strip_prefix(root).with_context(|| {
        format!(
            "{} is outside the project at {}",
            path.display(),
            root.display()
        )
    })?;
    Ok(relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/"))
}

/// Files changed relative to `HEAD` plus untracked files, relative to `root`.
fn changed_files(root: &Path) -> Result<Vec<String>> {
    let mut files = git(root, &["diff", "--name-only", "--relative", "HEAD"])?;
    files.extend(git(root, &["ls-files", "--others", "--exclude-standard"])?);
    files.sort();
    files.dedup();
    Ok(files)
}

fn git(root: &Path, args: &[&str]) -> Result<Vec<String>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .context("failed to run git")?;
    if !output.status.success() {
        bail!(
            "`git {}` failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(ToString::to_string)
        .collect())
}
