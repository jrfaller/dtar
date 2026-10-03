#![forbid(unsafe_code)]

use std::{
    collections::BTreeMap,
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
    /// One or more files or directories to archive
    #[arg(value_name = "SOURCE", num_args = 1.., required = true)]
    sources: Vec<PathBuf>,

    /// Output path (default for one source; required for multiple)
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Replace the output file if it already exists
    #[arg(short, long)]
    force: bool,

    /// Exclude archive-relative paths matching this glob (repeatable)
    #[arg(short, long, value_name = "PATTERN")]
    exclude: Vec<String>,

    /// Print the archive tree without creating an archive
    #[arg(long)]
    dry_run: bool,

    /// Hide the progress bar and all successful output
    #[arg(short, long)]
    quiet: bool,
}

#[derive(Default)]
struct TreeNode {
    kind: Option<dtar::ArchiveEntryKind>,
    children: BTreeMap<String, TreeNode>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("dtar: {error:#}");
        process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    let args = Args::parse();
    if args.sources.len() > 1 && args.output.is_none() {
        anyhow::bail!("--output is required when multiple source paths are provided");
    }
    let output = args
        .output
        .unwrap_or_else(|| default_output_path(&args.sources[0]));

    if args.dry_run {
        let plan = dtar::plan_sources(&args.sources, &output, args.force, &args.exclude)?;
        if !args.quiet {
            println!("Dry run: no archive will be created.");
            println!("Output: {}", plan.output_path().display());
            let root_name = if args.sources.len() == 1 && args.sources[0].is_dir() {
                args.sources[0]
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "Archive".to_owned())
            } else {
                "Archive".to_owned()
            };
            println!("{root_name}/");
            let mut tree = TreeNode::default();
            for entry in plan.entries() {
                let mut node = &mut tree;
                for component in entry.path.split('/') {
                    node = node.children.entry(component.to_owned()).or_default();
                }
                node.kind = Some(entry.kind);
            }
            print_tree(&tree, "");
        }
        return Ok(());
    }

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

    let stats = dtar::compress_sources_with_excludes_and_progress(
        &args.sources,
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

fn print_tree(node: &TreeNode, prefix: &str) {
    let child_count = node.children.len();
    for (index, (name, child)) in node.children.iter().enumerate() {
        let is_last = index + 1 == child_count;
        let connector = if is_last { "`-- " } else { "|-- " };
        let suffix = if child.kind == Some(dtar::ArchiveEntryKind::Directory) {
            "/"
        } else {
            ""
        };
        println!("{prefix}{connector}{name}{suffix}");

        let child_prefix = if is_last { "    " } else { "|   " };
        print_tree(child, &format!("{prefix}{child_prefix}"));
    }
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
