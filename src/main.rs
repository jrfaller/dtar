#![forbid(unsafe_code)]

use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process,
};

use anyhow::Context;
use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use tempfile::NamedTempFile;

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

    /// Replace the output archive and checksum manifest if they already exist
    #[arg(short, long)]
    force: bool,

    /// Write a SHA256SUMS manifest beside the archive
    #[arg(long)]
    checksum: bool,

    /// Exclude paths matching this glob; slashless patterns match at any depth
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

    let plan = if args.dry_run || args.checksum {
        Some(dtar::plan_sources(
            &args.sources,
            &output,
            args.force,
            &args.exclude,
        )?)
    } else {
        None
    };
    let checksum_manifest = if args.checksum {
        Some(checksum_manifest_path(
            plan.as_ref().expect("checksum option plans the archive"),
            args.force,
        )?)
    } else {
        None
    };

    if args.dry_run {
        let plan = plan.as_ref().expect("dry-run plans the archive");
        if !args.quiet {
            if args.checksum {
                println!("Dry run: no archive or checksum manifest will be created.");
            } else {
                println!("Dry run: no archive will be created.");
            }
            println!("Output: {}", plan.output_path().display());
            if let Some(manifest) = &checksum_manifest {
                println!("Checksum manifest: {}", manifest.display());
            }
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
    if let Some(manifest) = &checksum_manifest {
        write_checksum_manifest(manifest, &output, &stats.sha256, args.force)?;
    }
    if !args.quiet {
        println!("Created {}", output.display());
        if let Some(manifest) = &checksum_manifest {
            println!("Created checksum manifest {}", manifest.display());
        }
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

fn checksum_manifest_path(plan: &dtar::ArchivePlan, overwrite: bool) -> anyhow::Result<PathBuf> {
    let output = plan.output_path();
    let filename = output
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow::anyhow!("archive filename must be valid UTF-8 for SHA256SUMS"))?;
    if filename == "SHA256SUMS" {
        anyhow::bail!("archive output cannot also be named SHA256SUMS");
    }
    if filename
        .chars()
        .any(|character| matches!(character, '\n' | '\r' | '\\'))
    {
        anyhow::bail!("archive filename cannot be represented safely in SHA256SUMS");
    }

    let manifest = output
        .parent()
        .expect("resolved archive path has a parent")
        .join("SHA256SUMS");
    match fs::symlink_metadata(&manifest) {
        Ok(metadata) if metadata.file_type().is_dir() => {
            anyhow::bail!(
                "checksum manifest path is a directory: {}",
                manifest.display()
            )
        }
        Ok(_) if !overwrite => anyhow::bail!(
            "checksum manifest already exists (use --force to replace it): {}",
            manifest.display()
        ),
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error).context(format!(
                "cannot inspect checksum manifest {}",
                manifest.display()
            ));
        }
    }
    Ok(manifest)
}

fn write_checksum_manifest(
    manifest: &Path,
    archive: &Path,
    checksum: &str,
    overwrite: bool,
) -> anyhow::Result<()> {
    let filename = archive
        .file_name()
        .and_then(|name| name.to_str())
        .expect("checksum manifest path validates the archive filename");
    let parent = manifest
        .parent()
        .expect("resolved checksum manifest path has a parent");
    let mut temporary = NamedTempFile::new_in(parent).with_context(|| {
        format!(
            "cannot create temporary checksum manifest in {}",
            parent.display()
        )
    })?;
    writeln!(temporary, "{checksum}  {filename}")
        .with_context(|| format!("cannot write checksum manifest {}", manifest.display()))?;
    temporary.as_file().sync_all().with_context(|| {
        format!(
            "cannot synchronize checksum manifest {}",
            manifest.display()
        )
    })?;
    if overwrite {
        temporary
            .persist(manifest)
            .with_context(|| format!("cannot replace checksum manifest {}", manifest.display()))?;
    } else {
        temporary
            .persist_noclobber(manifest)
            .with_context(|| format!("cannot create checksum manifest {}", manifest.display()))?;
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
