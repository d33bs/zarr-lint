//! `zarr-lint` command line interface, exposed as a library so that both the
//! native binary and the Python bindings run exactly the same code.
//!
//! The entry point is [`run`], which parses arguments, executes the requested
//! command, and returns a process exit code. It never calls
//! [`std::process::exit`], so callers (including the Python extension) stay in
//! control of the process.
//!
//! Usage:
//!
//! ```text
//! zarr-lint check path/to/store.zarr     # primary form
//! zarr-lint path/to/store.zarr           # shorthand for `check`
//! zarr-lint fmt path/to/store.zarr       # preview metadata formatting
//! zarr-lint inspect path/to/store.zarr   # print a node summary
//! zarr-lint version --verbose            # detailed version info
//! zarr-lint --version                    # `zarr-lint 0.0.1`
//! ```
//!
//! Exit codes are stable (see the [`exit`] constants) so the tool can be used
//! in CI.

use std::ffi::OsString;

use clap::{Args, Parser, Subcommand, ValueEnum};
use zarr_lint_core::model::format_dims;
use zarr_lint_core::{
    format_store, lint_store_with, load_store, plan_format_store, FormatPlan, Report, Severity,
    StoreOptions,
};

const ABOUT: &str = "Inspect Zarr stores for structural and metadata problems.";

/// Stable process exit codes.
pub mod exit {
    /// No findings reached the failure threshold.
    pub const OK: i32 = 0;
    /// Lint findings reached the failure threshold.
    pub const FINDINGS: i32 = 1;
    /// Invalid command usage or configuration.
    pub const USAGE: i32 = 2;
    /// Store access or internal execution failure.
    pub const FAILURE: i32 = 3;
}

#[derive(Parser)]
#[command(
    name = "zarr-lint",
    version,
    about = ABOUT,
    args_conflicts_with_subcommands = true
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Arguments used for the shorthand form, `zarr-lint <PATH>`.
    #[command(flatten)]
    check: CheckArgs,
}

#[derive(Subcommand)]
enum Command {
    /// Check a Zarr store for structural and metadata problems.
    Check(CheckArgs),
    /// Format Zarr metadata without changing store semantics.
    Fmt(FmtArgs),
    /// Print a summary of the groups and arrays discovered in a store.
    Inspect(InspectArgs),
    /// Print detailed version information.
    Version(VersionArgs),
}

#[derive(Args, Clone)]
struct CheckArgs {
    /// Path or http(s):// URL of the Zarr store to check.
    path: Option<String>,

    /// Output format.
    #[arg(long, value_enum, default_value_t = Format::Text)]
    format: Format,

    /// Severity at or above which findings cause a non-zero exit.
    #[arg(long = "fail-on", value_enum, default_value_t = FailOn::Error)]
    fail_on: FailOn,

    /// Suppress the summary and success lines (text output only).
    #[arg(long)]
    quiet: bool,

    /// Access cloud object stores anonymously (no credentials or signing).
    #[arg(long)]
    anonymous: bool,
}

#[derive(Args, Clone)]
struct InspectArgs {
    /// Path or http(s):// URL of the Zarr store to inspect.
    path: String,

    /// Access cloud object stores anonymously (no credentials or signing).
    #[arg(long)]
    anonymous: bool,
}

#[derive(Args, Clone)]
struct FmtArgs {
    /// Local filesystem path of the Zarr store to format.
    path: String,

    /// Output format.
    #[arg(long, value_enum, default_value_t = Format::Text)]
    format: Format,

    /// Fail if formatting is needed, but do not write files.
    #[arg(long, conflicts_with = "write")]
    check: bool,

    /// Apply formatting changes.
    #[arg(long)]
    write: bool,
}

#[derive(Args, Clone)]
struct VersionArgs {
    /// Include git commit and build profile information.
    #[arg(long)]
    verbose: bool,
}

#[derive(Copy, Clone, ValueEnum)]
enum Format {
    /// Human-readable text.
    Text,
    /// Machine-readable JSON.
    Json,
}

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
enum FailOn {
    /// Fail on warnings and errors.
    Warning,
    /// Fail on errors only (default).
    Error,
    /// Never fail because of findings.
    Never,
}

/// Parse `args` (including the program name as the first element), run the
/// requested command, and return the process exit code.
///
/// Argument-parsing errors, `--help`, and `--version` are handled here: their
/// output is printed and the corresponding exit code (2 for usage errors, 0 for
/// help/version) is returned rather than terminating the process.
pub fn run<I, T>(args: I) -> i32
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(err) => {
            // Prints help/version to stdout, or the error to stderr.
            let _ = err.print();
            return err.exit_code();
        }
    };

    match cli.command {
        Some(Command::Check(args)) => run_check(args),
        Some(Command::Fmt(args)) => run_fmt(args),
        Some(Command::Inspect(args)) => run_inspect(args),
        Some(Command::Version(args)) => {
            print_version(args.verbose);
            exit::OK
        }
        None => run_check(cli.check),
    }
}

fn run_fmt(args: FmtArgs) -> i32 {
    if args.write {
        let plan = match format_store(&args.path) {
            Ok(plan) => plan,
            Err(err) => {
                eprintln!("error: {err}");
                return exit::FAILURE;
            }
        };
        match args.format {
            Format::Text => {
                if plan.is_empty() {
                    println!("All metadata files are formatted.");
                } else {
                    println!("Formatted {} metadata file(s).", plan.changes.len());
                    println!("Validated store after write.");
                }
            }
            Format::Json => print_fmt_json(&args.path, "write", &plan),
        }
        return exit::OK;
    }

    let plan = match plan_format_store(&args.path) {
        Ok(plan) => plan,
        Err(err) => {
            eprintln!("error: {err}");
            return exit::FAILURE;
        }
    };

    if args.check {
        match args.format {
            Format::Text => {
                if plan.is_empty() {
                    println!("All metadata files are formatted.");
                } else {
                    println!("{} metadata file(s) need formatting.", plan.changes.len());
                }
            }
            Format::Json => print_fmt_json(&args.path, "check", &plan),
        }
        if plan.is_empty() {
            exit::OK
        } else {
            exit::FINDINGS
        }
    } else if matches!(args.format, Format::Json) {
        print_fmt_json(&args.path, "dry-run", &plan);
        exit::OK
    } else {
        print_fmt_plan(&plan);
        exit::OK
    }
}

fn run_check(args: CheckArgs) -> i32 {
    let Some(path) = args.path else {
        eprintln!("error: a store path is required");
        eprintln!("\nUsage: zarr-lint check <PATH>");
        eprintln!("Run `zarr-lint --help` for more information.");
        return exit::USAGE;
    };

    let options = StoreOptions {
        anonymous: args.anonymous,
    };
    let report = match lint_store_with(&path, &options) {
        Ok(report) => report,
        Err(err) => {
            eprintln!("error: {err}");
            return exit::FAILURE;
        }
    };

    match args.format {
        Format::Text => print_text(&report, args.quiet),
        Format::Json => print_json(&report),
    }

    let failed = match args.fail_on {
        FailOn::Never => false,
        FailOn::Warning => report.has_at_or_above(Severity::Warning),
        FailOn::Error => report.has_at_or_above(Severity::Error),
    };
    if failed {
        exit::FINDINGS
    } else {
        exit::OK
    }
}

fn print_text(report: &Report, quiet: bool) {
    for diagnostic in &report.diagnostics {
        println!(
            "{}[{}] {}",
            diagnostic.severity, diagnostic.rule, diagnostic.path
        );
        println!("  {}", diagnostic.message);
        if let Some(detail) = &diagnostic.detail {
            println!();
            println!("  Caused by:");
            for line in detail.lines() {
                println!("    {line}");
            }
        }
        println!();
    }

    if quiet {
        return;
    }

    let (errors, warnings, infos) = report.counts();
    if report.diagnostics.is_empty() {
        println!("No problems found in {}.", report.store);
    } else {
        println!(
            "{} finding(s): {} error(s), {} warning(s), {} info.",
            report.diagnostics.len(),
            errors,
            warnings,
            infos
        );
    }
}

fn print_json(report: &Report) {
    match serde_json::to_string_pretty(report) {
        Ok(json) => println!("{json}"),
        // Serializing a Report cannot realistically fail, but never panic on it.
        Err(err) => eprintln!("error: failed to serialize report: {err}"),
    }
}

fn print_fmt_plan(plan: &FormatPlan) {
    if plan.is_empty() {
        println!("All metadata files are formatted.");
        return;
    }

    println!("Would format {} metadata file(s):", plan.changes.len());
    for change in &plan.changes {
        println!("  {}", change.rel_display);
    }
    println!();
    println!("No files were changed. Run with --write to apply.");
}

fn print_fmt_json(store: &str, mode: &str, plan: &FormatPlan) {
    let changes: Vec<&str> = plan
        .changes
        .iter()
        .map(|change| change.rel_display.as_str())
        .collect();
    let report = serde_json::json!({
        "version": zarr_lint_core::VERSION,
        "store": store,
        "mode": mode,
        "would_change": !plan.is_empty(),
        "changed_count": plan.changes.len(),
        "changes": changes,
    });
    match serde_json::to_string_pretty(&report) {
        Ok(json) => println!("{json}"),
        Err(err) => eprintln!("error: failed to serialize fmt report: {err}"),
    }
}

fn run_inspect(args: InspectArgs) -> i32 {
    let options = StoreOptions {
        anonymous: args.anonymous,
    };
    let (scan, loaded) = match load_store(&args.path, &options) {
        Ok(loaded) => loaded,
        Err(err) => {
            eprintln!("error: {err}");
            return exit::FAILURE;
        }
    };

    if !scan.is_recognized() {
        println!("No Zarr metadata found in {}.", args.path);
        return exit::OK;
    }

    println!("Store: {}", args.path);
    println!("Discovered {} metadata document(s):", scan.files.len());
    for node in &loaded.parsed {
        let kind = node.kind().map(|k| k.as_str()).unwrap_or("unknown");
        let location = if node.location.is_empty() {
            "/"
        } else {
            node.location.as_str()
        };
        let mut line = format!("  {} {:<7} {}", node.version, kind, location);
        if let Some(shape) = node.shape_dims() {
            line.push_str(&format!("  shape={}", format_dims(shape)));
        }
        if let Some(chunks) = node.chunk_dims() {
            line.push_str(&format!(" chunks={}", format_dims(chunks)));
        }
        println!("{line}");
    }
    for failure in &loaded.parse_failures {
        println!("  ! {} (invalid JSON)", failure.rel_display);
    }
    exit::OK
}

fn print_version(verbose: bool) {
    println!("zarr-lint {}", zarr_lint_core::VERSION);
    if verbose {
        println!("commit: {}", env!("ZARRLINT_GIT_COMMIT"));
        println!(
            "build profile: {}",
            if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            }
        );
    }
}
