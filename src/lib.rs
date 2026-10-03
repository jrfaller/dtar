#![forbid(unsafe_code)]

use std::{
    fs::{self, File},
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Component, Path, PathBuf},
    time::{Duration, Instant},
};

use anyhow::{bail, Context, Result};
use flate2::{Compression, GzBuilder};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use sha2::{Digest, Sha256};
use tar::{Builder, EntryType, Header};
use tempfile::NamedTempFile;

struct Entry {
    source: PathBuf,
    archive_path: String,
    kind: EntryKind,
    size: u64,
}

#[derive(Clone, Copy)]
enum EntryKind {
    File,
    Directory,
}

/// Summary statistics for a completed archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveStats {
    /// Lowercase hexadecimal SHA-256 checksum of the completed archive.
    pub sha256: String,
    /// Number of archived regular files.
    pub files: u64,
    /// Number of archived directories, excluding the source root.
    pub directories: u64,
    /// Total size of the source files before compression, in bytes.
    pub source_bytes: u64,
    /// Size of the completed `.tar.gz` archive, in bytes.
    pub archive_bytes: u64,
    /// Time from input validation through archive persistence.
    pub elapsed: Duration,
}

/// Compresses a directory into a deterministic tar.gz archive.
///
/// Existing output files are preserved. Set `overwrite` to `true` to replace
/// one atomically. Returns statistics for the completed archive.
pub fn compress_directory(
    source: impl AsRef<Path>,
    output: impl AsRef<Path>,
    overwrite: bool,
) -> Result<ArchiveStats> {
    compress_directory_with_excludes(source, output, overwrite, &[])
}

/// Compresses a directory while excluding entries matching any glob pattern.
///
/// Patterns match source-relative paths using `/` separators. A matching
/// directory and its contents are omitted. Returns statistics for the
/// completed archive.
pub fn compress_directory_with_excludes(
    source: impl AsRef<Path>,
    output: impl AsRef<Path>,
    overwrite: bool,
    exclude_patterns: &[String],
) -> Result<ArchiveStats> {
    compress_directory_with_excludes_and_progress(
        source,
        output,
        overwrite,
        exclude_patterns,
        |_, _| {},
    )
}

/// Compresses a directory, reporting completed and total archive entries.
///
/// Entries include directories as well as files. The callback is invoked once
/// before writing begins and once after each entry is written. Returns
/// statistics for the completed archive.
pub fn compress_directory_with_progress<F>(
    source: impl AsRef<Path>,
    output: impl AsRef<Path>,
    overwrite: bool,
    progress: F,
) -> Result<ArchiveStats>
where
    F: FnMut(u64, u64),
{
    compress_directory_with_excludes_and_progress(source, output, overwrite, &[], progress)
}

/// Compresses a directory with glob exclusions and progress reporting.
///
/// Patterns match source-relative paths using `/` separators. A matching
/// directory and its contents are omitted. The callback is invoked once before
/// writing begins and once after each included entry is written.
pub fn compress_directory_with_excludes_and_progress<F>(
    source: impl AsRef<Path>,
    output: impl AsRef<Path>,
    overwrite: bool,
    exclude_patterns: &[String],
    mut progress: F,
) -> Result<ArchiveStats>
where
    F: FnMut(u64, u64),
{
    let started = Instant::now();
    let source = fs::canonicalize(source.as_ref()).with_context(|| {
        format!(
            "cannot access source directory {}",
            source.as_ref().display()
        )
    })?;
    if !source.is_dir() {
        bail!("source is not a directory: {}", source.display());
    }

    let output = resolve_output_path(output.as_ref())?;
    if output.starts_with(&source) {
        bail!(
            "output archive must be outside the source directory: {}",
            output.display()
        );
    }
    if output.exists() && !overwrite {
        bail!(
            "output already exists (use --force to replace it): {}",
            output.display()
        );
    }

    let excludes = build_excludes(exclude_patterns)?;
    let mut entries = collect_entries(&source, &excludes)?;
    entries.sort_by(|left, right| left.archive_path.cmp(&right.archive_path));
    let total = entries.len() as u64;
    let files = entries
        .iter()
        .filter(|entry| matches!(entry.kind, EntryKind::File))
        .count() as u64;
    let directories = entries
        .iter()
        .filter(|entry| matches!(entry.kind, EntryKind::Directory))
        .count() as u64;
    let source_bytes = entries
        .iter()
        .filter(|entry| matches!(entry.kind, EntryKind::File))
        .try_fold(0_u64, |total, entry| total.checked_add(entry.size))
        .context("total source file size exceeds the supported range")?;
    progress(0, total);

    let parent = output
        .parent()
        .context("output archive has no parent directory")?;
    let temporary = NamedTempFile::new_in(parent)
        .with_context(|| format!("cannot create temporary archive in {}", parent.display()))?;

    let gzip = GzBuilder::new()
        .mtime(0)
        .operating_system(255)
        .write(temporary, Compression::best());
    let mut archive = Builder::new(gzip);
    archive.mode(tar::HeaderMode::Deterministic);

    for (index, entry) in entries.iter().enumerate() {
        let mut header = Header::new_gnu();
        header.set_mtime(0);
        header.set_uid(0);
        header.set_gid(0);
        header.set_username("")?;
        header.set_groupname("")?;
        header.set_mode(match entry.kind {
            EntryKind::File => 0o644,
            EntryKind::Directory => 0o755,
        });
        header.set_size(entry.size);
        header.set_entry_type(match entry.kind {
            EntryKind::File => EntryType::Regular,
            EntryKind::Directory => EntryType::Directory,
        });
        header.set_path(&entry.archive_path).with_context(|| {
            format!("archive path cannot be represented: {}", entry.archive_path)
        })?;
        header.set_cksum();

        match entry.kind {
            EntryKind::Directory => archive
                .append(&header, io::empty())
                .with_context(|| format!("cannot archive directory {}", entry.archive_path))?,
            EntryKind::File => {
                let mut file = File::open(&entry.source)
                    .with_context(|| format!("cannot read {}", entry.source.display()))?;
                let metadata = file
                    .metadata()
                    .with_context(|| format!("cannot inspect {}", entry.source.display()))?;
                if !metadata.is_file() || metadata.len() != entry.size {
                    bail!("source changed while archiving: {}", entry.source.display());
                }
                archive
                    .append(&header, &mut file)
                    .with_context(|| format!("cannot archive file {}", entry.archive_path))?;
            }
        }
        progress(index as u64 + 1, total);
    }

    let gzip = archive
        .into_inner()
        .context("cannot finalize tar archive")?;
    let mut temporary = gzip.finish().context("cannot finalize gzip stream")?;
    temporary
        .as_file_mut()
        .flush()
        .context("cannot flush completed archive")?;
    temporary
        .as_file()
        .sync_all()
        .context("cannot synchronize completed archive")?;

    let archive_bytes = temporary
        .as_file()
        .metadata()
        .context("cannot inspect completed archive")?
        .len();
    let checksum = sha256(temporary.as_file_mut()).context("cannot hash completed archive")?;
    if overwrite {
        temporary
            .persist(&output)
            .with_context(|| format!("cannot replace output archive {}", output.display()))?;
    } else {
        temporary
            .persist_noclobber(&output)
            .with_context(|| format!("cannot create output archive {}", output.display()))?;
    }

    Ok(ArchiveStats {
        sha256: checksum,
        files,
        directories,
        source_bytes,
        archive_bytes,
        elapsed: started.elapsed(),
    })
}

fn build_excludes(patterns: &[String]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        if pattern.is_empty() {
            bail!("exclude pattern must not be empty");
        }
        if pattern.starts_with('/')
            || pattern.contains('\\')
            || pattern.as_bytes().get(1) == Some(&b':')
            || pattern.split('/').any(|component| component == "..")
        {
            bail!("exclude patterns must be source-relative and use '/' separators: {pattern:?}");
        }

        let glob = GlobBuilder::new(pattern)
            .literal_separator(false)
            .build()
            .with_context(|| format!("invalid exclude pattern {pattern:?}"))?;
        builder.add(glob);
    }
    builder.build().context("cannot compile exclude patterns")
}

fn collect_entries(root: &Path, excludes: &GlobSet) -> Result<Vec<Entry>> {
    let mut entries = Vec::new();
    let mut pending = vec![root.to_path_buf()];

    while let Some(directory) = pending.pop() {
        let mut children = fs::read_dir(&directory)
            .with_context(|| format!("cannot read directory {}", directory.display()))?
            .map(|result| result.map(|entry| entry.path()))
            .collect::<io::Result<Vec<_>>>()
            .with_context(|| format!("cannot list directory {}", directory.display()))?;
        children.sort();

        for path in children {
            let metadata = fs::symlink_metadata(&path)
                .with_context(|| format!("cannot inspect {}", path.display()))?;
            let relative = path
                .strip_prefix(root)
                .with_context(|| format!("cannot make {} relative to source", path.display()))?;
            let archive_path = normalize_path(relative)?;

            if excludes.is_match(&archive_path) {
                continue;
            }
            if metadata.file_type().is_symlink() {
                bail!("symbolic links are not supported: {}", path.display());
            }
            if metadata.is_dir() {
                entries.push(Entry {
                    source: path.clone(),
                    archive_path,
                    kind: EntryKind::Directory,
                    size: 0,
                });
                pending.push(path);
            } else if metadata.is_file() {
                entries.push(Entry {
                    source: path,
                    archive_path,
                    kind: EntryKind::File,
                    size: metadata.len(),
                });
            } else {
                bail!("unsupported filesystem entry: {}", path.display());
            }
        }
    }

    Ok(entries)
}

fn normalize_path(path: &Path) -> Result<String> {
    let mut normalized = String::new();
    for component in path.components() {
        let Component::Normal(part) = component else {
            bail!("invalid relative archive path: {}", path.display());
        };
        let part = part
            .to_str()
            .with_context(|| format!("path is not valid UTF-8: {}", path.display()))?;
        if !normalized.is_empty() {
            normalized.push('/');
        }
        normalized.push_str(part);
    }
    if normalized.is_empty() {
        bail!("archive entry path cannot be empty");
    }
    Ok(normalized)
}

fn resolve_output_path(output: &Path) -> Result<PathBuf> {
    let parent = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent = fs::canonicalize(parent)
        .with_context(|| format!("cannot access output directory {}", parent.display()))?;
    let filename = output.file_name().context("output path must name a file")?;
    let candidate = parent.join(filename);
    if candidate.exists() {
        fs::canonicalize(&candidate)
            .with_context(|| format!("cannot resolve output path {}", candidate.display()))
    } else {
        Ok(candidate)
    }
}

fn sha256(file: &mut File) -> Result<String> {
    file.seek(SeekFrom::Start(0))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}
