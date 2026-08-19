//! `wgcheck`: read WGCheck `.gch` report archives and pull the python logs.

use std::fs;
use std::path::Path;
use std::path::PathBuf;

use clap::Parser;
use clap::Subcommand;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use wgcheck::Member;
use wgcheck::parse_gch;

#[derive(Parser)]
#[command(about = "Parse WGCheck (.gch) reports and extract python logs")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Print the root object members and a summary of captured logs.
    Dump { file: PathBuf },
    /// Write the python.log variants from each file to <outdir>.
    Extract {
        outdir: PathBuf,
        #[arg(required = true)]
        files: Vec<PathBuf>,
    },
    /// Search the python logs in each file for a substring; print matching lines.
    Grep {
        pattern: String,
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Lines of context to print before and after each match.
        #[arg(short = 'C', long, default_value_t = 0)]
        context: usize,
    },
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Dump { file } => dump(&file),
        Cmd::Extract { outdir, files } => extract(&outdir, &files),
        Cmd::Grep { pattern, files, context } => grep(&pattern, &files, context),
    }
}

fn read_report(file: &Path) -> Result<wgcheck::Report> {
    let bytes = fs::read(file).context_with(|| format!("reading {}", file.display()))?;
    Ok(parse_gch(&bytes).context_with(|| format!("parsing {}", file.display()))?)
}

fn dump(file: &Path) -> Result<()> {
    let report = read_report(file)?;
    println!("file:  {}", file.display());
    println!("class: {}", report.class_name);
    if let Some(name) = report.str("ClientName") {
        println!("ClientName: {name}");
    }
    println!("members:");
    for (name, value) in &report.members {
        let desc = match value {
            Member::Null => "null".to_string(),
            Member::Bool(b) => format!("bool {b}"),
            Member::Int(i) => format!("int {i}"),
            Member::Float(f) => format!("float {f}"),
            Member::Str(s) if s.len() <= 64 => format!("string {s:?}"),
            Member::Str(s) => format!("string ({} bytes)", s.len()),
            Member::Other(s) => s.clone(),
        };
        println!("  {name:<28} {desc}");
    }
    println!("python logs:");
    for (label, text) in report.python_logs() {
        println!("  {label:<18} {} bytes", text.len());
    }
    Ok(())
}

fn extract(outdir: &Path, files: &[PathBuf]) -> Result<()> {
    fs::create_dir_all(outdir).context_with(|| format!("creating {}", outdir.display()))?;
    for file in files {
        let report = match read_report(file) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("skip {}: {e:#}", file.display());
                continue;
            }
        };
        let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("report");
        let logs = report.python_logs();
        if logs.is_empty() {
            println!("{}: no python logs", file.display());
            continue;
        }
        for (label, text) in logs {
            let suffix = label.replace(['/', '\\'], "_");
            let out = outdir.join(format!("{stem}.{suffix}"));
            fs::write(&out, text).context_with(|| format!("writing {}", out.display()))?;
            println!("{} -> {} ({} bytes)", file.display(), out.display(), text.len());
        }
    }
    Ok(())
}

fn grep(pattern: &str, files: &[PathBuf], context: usize) -> Result<()> {
    let mut total = 0usize;
    for file in files {
        let report = match read_report(file) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("skip {}: {e:#}", file.display());
                continue;
            }
        };
        let name = file.file_name().and_then(|s| s.to_str()).unwrap_or("?");
        for (label, text) in report.python_logs() {
            let lines: Vec<&str> = text.lines().collect();
            for (i, line) in lines.iter().enumerate() {
                if line.contains(pattern) {
                    total += 1;
                    let lo = i.saturating_sub(context);
                    let hi = (i + context + 1).min(lines.len());
                    for (j, ctx) in lines[lo..hi].iter().enumerate() {
                        let marker = if lo + j == i { ">" } else { " " };
                        println!("{name} [{label}:{}] {marker} {ctx}", lo + j + 1);
                    }
                    if context > 0 {
                        println!("--");
                    }
                }
            }
        }
    }
    eprintln!("{total} matching line(s)");
    Ok(())
}
