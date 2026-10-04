#![forbid(unsafe_code)]

use std::{
    collections::BTreeSet,
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
    kind: ArchiveEntryKind,
    size: u64,
}

struct SourceInput {
    path: PathBuf,
    is_directory: bool,
}

/// The kind of an entry in an archive plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArchiveEntryKind {
    File,
    Directory,
}

/// A read-only view of one entry in an archive plan.
#[derive(Clone, Copy, Debug)]
pub struct PlannedEntry<'a> {
    /// Normalized path relative to the source directory.
    pub path: &'a str,
    /// Whether this entry is a file or directory.
    pub kind: ArchiveEntryKind,
    /// File size in bytes; directory sizes are zero.
    pub size: u64,
}

/// Validated archive contents and destination, without creating an archive.
pub struct ArchivePlan {
    output: PathBuf,
    entries: Vec<Entry>,
}

impl ArchivePlan {
    /// Returns the resolved destination path for this plan.
    pub fn output_path(&self) -> &Path {
        &self.output
    }

    /// Iterates over the archive entries in their deterministic write order.
    pub fn entries(&self) -> impl Iterator<Item = PlannedEntry<'_>> {
        self.entries.iter().map(|entry| PlannedEntry {
            path: &entry.archive_path,
            kind: entry.kind,
            size: entry.size,
        })
    }
}

/// Summary statistics for a completed archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveStats {
    /// Lowercase hexadecimal SHA-256 checksum of the completed archive.
    pub sha256: String,
    /// Number of archived regular files.
    pub files: u64,
    /// Number of archived directory entries.
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
/// Patterns match archive-relative paths using `/` separators. Patterns
/// without `/` match path components at any depth; `*` does not match `/`, but
/// `**` does. A matching directory and its contents are omitted. Returns
/// statistics for the completed archive.
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
/// Patterns match archive-relative paths using `/` separators. Patterns
/// without `/` match path components at any depth; `*` does not match `/`, but
/// `**` does. A matching directory and its contents are omitted. The callback
/// is invoked once before writing begins and once after each included entry is
/// written.
pub fn compress_directory_with_excludes_and_progress<F>(
    source: impl AsRef<Path>,
    output: impl AsRef<Path>,
    overwrite: bool,
    exclude_patterns: &[String],
    progress: F,
) -> Result<ArchiveStats>
where
    F: FnMut(u64, u64),
{
    let source = source.as_ref();
    ensure_directory_source(source)?;
    compress_sources_with_excludes_and_progress(
        &[source.to_path_buf()],
        output,
        overwrite,
        exclude_patterns,
        progress,
    )
}

/// Compresses one or more files and directories into a deterministic tar.gz
/// archive. A single directory retains the directory-only layout; multiple
/// inputs are stored under their basenames.
pub fn compress_sources(
    sources: &[PathBuf],
    output: impl AsRef<Path>,
    overwrite: bool,
) -> Result<ArchiveStats> {
    compress_sources_with_excludes(sources, output, overwrite, &[])
}

/// Compresses one or more files and directories while excluding entries
/// matching archive-relative glob patterns. Patterns without `/` match path
/// components at any depth; patterns containing `/` are relative to the
/// archive root.
pub fn compress_sources_with_excludes(
    sources: &[PathBuf],
    output: impl AsRef<Path>,
    overwrite: bool,
    exclude_patterns: &[String],
) -> Result<ArchiveStats> {
    compress_sources_with_excludes_and_progress(
        sources,
        output,
        overwrite,
        exclude_patterns,
        |_, _| {},
    )
}

/// Compresses one or more files and directories with exclusions and progress
/// reporting. A single directory retains the directory-only layout; with
/// multiple inputs, each input is stored under its basename. Patterns without
/// `/` match path components at any depth; patterns containing `/` are
/// relative to the archive root. `*` and `?` do not match `/`, but `**` does.
pub fn compress_sources_with_excludes_and_progress<F>(
    sources: &[PathBuf],
    output: impl AsRef<Path>,
    overwrite: bool,
    exclude_patterns: &[String],
    mut progress: F,
) -> Result<ArchiveStats>
where
    F: FnMut(u64, u64),
{
    let started = Instant::now();
    let plan = plan_sources(sources, output, overwrite, exclude_patterns)?;
    let ArchivePlan { output, entries } = plan;
    let total = entries.len() as u64;
    let files = entries
        .iter()
        .filter(|entry| matches!(entry.kind, ArchiveEntryKind::File))
        .count() as u64;
    let directories = entries
        .iter()
        .filter(|entry| matches!(entry.kind, ArchiveEntryKind::Directory))
        .count() as u64;
    let source_bytes = entries
        .iter()
        .filter(|entry| matches!(entry.kind, ArchiveEntryKind::File))
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
            ArchiveEntryKind::File => 0o644,
            ArchiveEntryKind::Directory => 0o755,
        });
        header.set_size(entry.size);
        header.set_entry_type(match entry.kind {
            ArchiveEntryKind::File => EntryType::Regular,
            ArchiveEntryKind::Directory => EntryType::Directory,
        });
        header.set_path(&entry.archive_path).with_context(|| {
            format!("archive path cannot be represented: {}", entry.archive_path)
        })?;
        header.set_cksum();

        match entry.kind {
            ArchiveEntryKind::Directory => archive
                .append(&header, io::empty())
                .with_context(|| format!("cannot archive directory {}", entry.archive_path))?,
            ArchiveEntryKind::File => {
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
            bail!("exclude patterns must be archive-relative and use '/' separators: {pattern:?}");
        }

        let glob_pattern = if pattern.contains('/') {
            pattern.clone()
        } else {
            format!("**/{pattern}")
        };
        let glob = GlobBuilder::new(&glob_pattern)
            .literal_separator(true)
            .build()
            .with_context(|| format!("invalid exclude pattern {pattern:?}"))?;
        builder.add(glob);
    }
    builder.build().context("cannot compile exclude patterns")
}

/// Plans an archive without creating or modifying the destination file.
///
/// The single directory, destination, overwrite policy, and exclusion
/// patterns are validated exactly as they are for compression. The directory
/// contents are archived without a top-level directory entry.
pub fn plan_archive(
    source: impl AsRef<Path>,
    output: impl AsRef<Path>,
    overwrite: bool,
    exclude_patterns: &[String],
) -> Result<ArchivePlan> {
    let source = source.as_ref();
    ensure_directory_source(source)?;
    plan_sources(&[source.to_path_buf()], output, overwrite, exclude_patterns)
}

/// Plans an archive from one or more files or directories without creating or
/// modifying the destination file. With one directory, its contents are
/// archived without the directory's basename. Otherwise, each input is stored
/// under its basename.
pub fn plan_sources(
    sources: &[PathBuf],
    output: impl AsRef<Path>,
    overwrite: bool,
    exclude_patterns: &[String],
) -> Result<ArchivePlan> {
    let sources = canonicalize_sources(sources)?;
    let output = resolve_output_path(output.as_ref())?;
    for source in &sources {
        if source.is_directory && output.starts_with(&source.path) {
            bail!(
                "output archive must be outside source directory {}: {}",
                source.path.display(),
                output.display()
            );
        }
        if !source.is_directory && output == source.path {
            bail!(
                "output archive cannot replace source file: {}",
                source.path.display()
            );
        }
    }
    if output.is_dir() {
        bail!("output archive path is a directory: {}", output.display());
    }
    if output.exists() && !overwrite {
        bail!(
            "output already exists (use --force to replace it): {}",
            output.display()
        );
    }

    let excludes = build_excludes(exclude_patterns)?;
    let include_directory_roots = sources.len() > 1;
    if include_directory_roots {
        let mut basenames = BTreeSet::new();
        for source in &sources {
            let basename = source_basename(&source.path)?;
            if !basenames.insert(basename.clone()) {
                bail!("multiple source paths map to the same archive path: {basename}");
            }
        }
    }
    let mut entries = Vec::new();
    for source in sources {
        if source.is_directory {
            let archive_prefix = if include_directory_roots {
                Some(source_basename(&source.path)?)
            } else {
                None
            };
            entries.extend(collect_directory_entries(
                &source.path,
                archive_prefix.as_deref(),
                &excludes,
            )?);
        } else {
            let archive_path = source_basename(&source.path)?;
            if !excludes.is_match(&archive_path) {
                let metadata = fs::metadata(&source.path)
                    .with_context(|| format!("cannot inspect {}", source.path.display()))?;
                entries.push(Entry {
                    source: source.path,
                    archive_path,
                    kind: ArchiveEntryKind::File,
                    size: metadata.len(),
                });
            }
        }
    }
    entries.sort_by(|left, right| left.archive_path.cmp(&right.archive_path));
    for pair in entries.windows(2) {
        if pair[0].archive_path == pair[1].archive_path {
            bail!(
                "multiple source paths map to the same archive path: {}",
                pair[0].archive_path
            );
        }
    }

    Ok(ArchivePlan { output, entries })
}

fn ensure_directory_source(source: &Path) -> Result<()> {
    let metadata = fs::metadata(source)
        .with_context(|| format!("cannot access source {}", source.display()))?;
    if !metadata.is_dir() {
        bail!("source is not a directory: {}", source.display());
    }
    Ok(())
}

fn canonicalize_sources(sources: &[PathBuf]) -> Result<Vec<SourceInput>> {
    if sources.is_empty() {
        bail!("at least one source path is required");
    }

    sources
        .iter()
        .map(|source| {
            let metadata = fs::symlink_metadata(source)
                .with_context(|| format!("cannot access source {}", source.display()))?;
            if metadata.file_type().is_symlink() {
                bail!("symbolic links are not supported: {}", source.display());
            }
            if !metadata.is_dir() && !metadata.is_file() {
                bail!("unsupported filesystem entry: {}", source.display());
            }
            let path = fs::canonicalize(source)
                .with_context(|| format!("cannot resolve source {}", source.display()))?;
            Ok(SourceInput {
                path,
                is_directory: metadata.is_dir(),
            })
        })
        .collect()
}

fn source_basename(source: &Path) -> Result<String> {
    source
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned)
        .with_context(|| format!("source path has no UTF-8 basename: {}", source.display()))
}

fn collect_directory_entries(
    root: &Path,
    archive_prefix: Option<&str>,
    excludes: &GlobSet,
) -> Result<Vec<Entry>> {
    let mut entries = Vec::new();
    if let Some(archive_path) = archive_prefix {
        if excludes.is_match(archive_path) {
            return Ok(entries);
        }
        entries.push(Entry {
            source: root.to_path_buf(),
            archive_path: archive_path.to_owned(),
            kind: ArchiveEntryKind::Directory,
            size: 0,
        });
    }

    let mut pending = vec![root.to_path_buf()];

    while let Some(directory) = pending.pop() {
        let mut children = fs::read_dir(&directory)
            .with_context(|| format!("cannot read directory {}", directory.display()))?
            .map(|result| result.map(|entry| entry.path()))
            .collect::<io::Result<Vec<_>>>()
            .with_context(|| format!("cannot list directory {}", directory.display()))?;
        children.sort();

        for path in children {
            let relative = path
                .strip_prefix(root)
                .with_context(|| format!("cannot make {} relative to source", path.display()))?;
            let archive_relative_path = normalize_path(relative)?;
            let archive_path = archive_prefix.map_or_else(
                || archive_relative_path.clone(),
                |prefix| format!("{prefix}/{archive_relative_path}"),
            );

            if excludes.is_match(&archive_path) {
                continue;
            }
            let metadata = fs::symlink_metadata(&path)
                .with_context(|| format!("cannot inspect {}", path.display()))?;
            if metadata.file_type().is_symlink() {
                bail!("symbolic links are not supported: {}", path.display());
            }
            if metadata.is_dir() {
                entries.push(Entry {
                    source: path.clone(),
                    archive_path,
                    kind: ArchiveEntryKind::Directory,
                    size: 0,
                });
                pending.push(path);
            } else if metadata.is_file() {
                entries.push(Entry {
                    source: path,
                    archive_path,
                    kind: ArchiveEntryKind::File,
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
