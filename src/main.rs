#![forbid(unsafe_code)]

use std::{
    path::{Path, PathBuf},
    process,
};

use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};

#[derive(Debug, Parser)]
#[command(
    name = "dtar",
    version,
    about = "Create deterministic .tar.gz archives"
)]
struct Args {
    /// Directory to archive
    source: PathBuf,

    /// Output archive path (defaults to <source-name>.tar.gz)
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Replace the output file if it already exists
    #[arg(short, long)]
    force: bool,

    /// Exclude source-relative paths matching this glob (repeatable)
    #[arg(short, long, value_name = "PATTERN")]
    exclude: Vec<String>,

    /// Hide the progress bar and successful-completion summary
    #[arg(short, long)]
    quiet: bool,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("dtar: {error:#}");
        process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    let args = Args::parse();
    let output = args
        .output
        .unwrap_or_else(|| default_output_path(&args.source));

    let progress = if args.quiet {
        ProgressBar::hidden()
    } else {
        let bar = ProgressBar::new(0);
        bar.set_style(
            ProgressStyle::with_template(
                "{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} entries",
            )
            .expect("static progress template is valid")
            .progress_chars("=>-"),
        );
        bar
    };

    let stats = dtar::compress_directory_with_excludes_and_progress(
        &args.source,
        &output,
        args.force,
        &args.exclude,
        |completed, total| {
            progress.set_length(total);
            progress.set_position(completed);
        },
    )?;
    progress.finish_and_clear();
    if !args.quiet {
        println!("Created {}", output.display());
        println!(
            "Files: {} | Directories: {}\nSource size: {} bytes | Archive size: {} bytes\nElapsed: {:?}\nSHA-256: {}",
            stats.files,
            stats.directories,
            stats.source_bytes,
            stats.archive_bytes,
            stats.elapsed,
            stats.sha256
        );
    }
    Ok(())
}

fn default_output_path(source: &Path) -> PathBuf {
    let name = source
        .file_name()
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| std::ffi::OsStr::new("archive"));
    let mut output_name = name.to_os_string();
    output_name.push(".tar.gz");
    source
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .join(output_name)
}
