# dtar

`dtar` creates reproducible `.tar.gz` archives from directories. File and
directory entries are sorted by normalized relative path; tar timestamps,
owners, names, and permissions are fixed, and the gzip timestamp and operating
system fields are normalized. Compression uses the Rust DEFLATE backend and a
fixed best-compression profile.

## Why deterministic archives?

Ordinary archive tools can produce different bytes for the same file contents
because timestamps, ownership, permissions, or filesystem traversal order
changed. `dtar` normalizes these details so that archiving the same supported
directory contents with the same tool version and options produces the same
archive bytes and SHA-256 checksum.

This is useful for:

- **Build pipelines and caches:** identical inputs produce identical artifacts,
  avoiding cache misses and unnecessary rebuilds.
- **Releases and mirrors:** compare checksums to confirm that independently
  produced or copied packages are byte-for-byte identical.
- **Backups and content-addressed storage:** stable hashes make unchanged
  snapshots easier to identify and deduplicate.
- **Git-tracked archives:** identical reruns leave the committed archive
  unchanged, avoiding timestamp-only diffs and allowing Git to reuse its blob.
- **Reproducible packaging:** rerun packaging and compare the result against a
  checksum from a trusted source. A checksum alone does not authenticate an
  archive; use a trusted or signed checksum when authenticity matters.

The tool archives regular files and directories. Symbolic links and other
special filesystem entries are rejected rather than followed. Archive paths
must be valid UTF-8. Output archives must be outside the source directory.

## Usage

```text
dtar [OPTIONS] <SOURCE>
```

`<SOURCE>` is the directory to archive. The output defaults to a sibling named
`<source-name>.tar.gz`; for example, archiving `./my-project` creates
`./my-project.tar.gz`.

### `<SOURCE>`

Required positional argument: the directory whose files and subdirectories
should be archived. The source root itself is not added as an entry.

```sh
dtar ./my-project
```

### `-o, --output <OUTPUT>`

Choose the output archive path instead of the default sibling path. The output
must be outside the source directory, and its parent directory must already
exist. Existing output files are not replaced unless `--force` is also given.

```sh
dtar ./my-project --output ./dist/my-project.tar.gz
```

### `-f, --force`

Replace an existing output archive atomically. Use this when rerunning a
command that writes to the same output path. The existing archive is kept if
compression fails; a partial archive is never published at the output path.

```sh
dtar ./my-project --output ./dist/my-project.tar.gz --force
```

### `-e, --exclude <PATTERN>`

Exclude paths matching a source-relative glob. This option can be repeated.
Patterns use `/` separators, and `*` can match across directory boundaries.
When a directory matches, it and its entire subtree are omitted. Nothing is
excluded by default.

```sh
dtar ./my-project \
  --exclude .git \
  --exclude target \
  --exclude '*.tmp'
```

### `--dry-run`

Validate the source, output path, and exclusions, then print a tree of entries
that would be archived without creating or replacing the output. Exclusions are
applied to the preview. Destination checks still apply; use `--force` to preview
an output path that already exists. The existing file will remain untouched. If
combined with `--quiet`, the tree is hidden.

```sh
dtar ./my-project \
  --output ./dist/my-project.tar.gz \
  --exclude .git \
  --exclude '*.tmp' \
  --dry-run
```

### `-q, --quiet`

Hide the progress bar and all successful output. The archive is still created
unless `--dry-run` is set, and errors are still reported.

```sh
dtar ./my-project --quiet
```

### `-h, --help`

Display the command syntax and all available options without starting a
compression.

```sh
dtar --help
```

### `-V, --version`

Print the installed `dtar` version.

```sh
dtar --version
```

### Complete example

This writes an archive to `./dist`, excludes generated and temporary content,
and replaces the archive if it already exists. Ensure `./dist` exists before
running the command.

```sh
dtar ./my-project \
  --output ./dist/my-project.tar.gz \
  --force \
  --exclude .git \
  --exclude target \
  --exclude '*.tmp'
```

Unless `--quiet` is set, successful compression prints the file and directory
counts, source and archive sizes, elapsed time, and archive SHA-256 checksum.

## Build and test

```sh
cargo build --release
cargo test
```

## Releases

Push a stable `vMAJOR.MINOR.PATCH` tag to run CI and publish a GitHub Release
with packages for Linux, macOS, and Windows on x86_64 and ARM64. Unix packages
are `.tar.gz` archives; Windows packages are `.zip` files. Each release also
includes `SHA256SUMS` to verify the packages.

```sh
git tag v0.0.1
git push origin v0.0.1
```

After downloading the manifest and packages into one directory, verify them
with:

```sh
sha256sum --check SHA256SUMS
```

On macOS, use `shasum -a 256 -c SHA256SUMS` instead.

## License

This project is licensed under the MIT License. See [LICENSE](LICENSE) for the
full text.
