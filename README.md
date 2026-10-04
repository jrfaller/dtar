# dtar

`dtar` creates reproducible `.tar.gz` archives from one or more files and
directories. File and directory entries are sorted by normalized archive path;
tar timestamps, owners, and names are fixed, while permissions are normalized
and executable status is preserved where the source filesystem exposes it. The
gzip timestamp and operating system fields are normalized. Compression uses the
Rust DEFLATE backend and a fixed best-compression profile.

## Why deterministic archives?

Ordinary archive tools can produce different bytes for the same file contents
because timestamps, ownership, permissions, or filesystem traversal order
changed. `dtar` normalizes these details so that archiving the same supported
input paths, contents, and executable status with the same tool version and
options produces the same archive bytes and SHA-256 checksum.

Regular files with any executable bit set are archived with mode `0o755`;
other regular files use `0o644`, and directories use `0o755`. Other source
permission bits are ignored. On filesystems that do not expose an executable
bit through file permissions, regular files use `0o644`.

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
must be valid UTF-8. The output archive must be outside all input directories
and must not replace an input file.

## Usage

```text
dtar [OPTIONS] <SOURCE>...
```

Provide one or more input paths, each of which can be a regular file or
directory. For one input, the output defaults to a sibling named
`<source-name>.tar.gz`; for example, archiving `./my-project` creates
`./my-project.tar.gz`. With multiple inputs, `--output` is required.

### `<SOURCE>...`

Required positional argument: one or more files or directories to archive. With
one directory, its contents are archived without the source directory itself,
preserving the previous layout. A single file is stored under its basename.
With multiple inputs, each is stored under its basename; directories include
their basename and all descendants. Inputs with colliding archive paths are
rejected, and `--output` must be specified. Exclude patterns match the
resulting archive-relative paths.

```sh
dtar ./my-project
```

```sh
dtar --output ./bundle.tar.gz ./my-project/src/main.rs ./LICENSE ./assets
```

This archives `main.rs`, `LICENSE`, and the `assets` directory recursively.

### `-o, --output <OUTPUT>`

Choose the output archive path. This option is required when providing multiple
inputs. The output must be outside every input directory and cannot replace a
file input; its parent directory must already exist. Existing output files are
not replaced unless `--force` is also given.

```sh
dtar ./my-project --output ./dist/my-project.tar.gz
```

### `-f, --force`

Replace an existing output archive and, when `--checksum` is set, an existing
`SHA256SUMS` manifest. Use this when rerunning a command that writes to the same
output paths. The existing archive is kept if compression fails; a partial
archive is never published at the output path.

```sh
dtar ./my-project --output ./dist/my-project.tar.gz --force
```

### `--checksum`

Write a `SHA256SUMS` manifest beside the archive after successful compression.
The manifest contains the archive's SHA-256 and filename, so it can be checked
from the directory containing the manifest. If `SHA256SUMS` already exists,
`--force` is required. With `--dry-run`, the manifest path is validated and
shown but no files are created.

```sh
dtar ./my-project --output ./dist/my-project.tar.gz --checksum
```

Verify the archive from `./dist` with:

```sh
cd ./dist && sha256sum --check SHA256SUMS
```

On macOS, use `shasum -a 256 -c SHA256SUMS` instead.

### `-e, --exclude <PATTERN>`

Exclude archive-relative paths matching a glob. This option can be repeated.
Patterns without `/` match files or directories with that name at any depth,
so `.DS_Store` excludes nested matches. Patterns containing `/` are relative to
the archive root. `*` and `?` do not match `/`; use `**` to match across
directory levels. When a directory matches, it and its entire subtree are
omitted. Nothing is excluded by default.

```sh
dtar ./my-project \
  --exclude .git \
  --exclude target \
  --exclude 'temp/*.tmp' \
  --exclude '*.tmp'
```

Here, `temp/*.tmp` matches `.tmp` files directly inside the root-level
`temp/` directory, while `*.tmp` matches `.tmp` files at any depth.

### `--dry-run`

Validate all source paths, the output path, and exclusions, then print a tree of
entries that would be archived without creating or replacing the output. All
sources and exclusions are applied to the preview. Destination checks still
apply; use `--force` to preview an output path that already exists. The existing
file will remain untouched. If combined with `--quiet`, the tree is hidden.

```sh
dtar \
  --dry-run \
  --output ./dist/my-project.tar.gz \
  --exclude .git \
  --exclude '*.tmp' \
  ./my-project \
  ./assets
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

This writes an archive and checksum manifest to `./dist`, excludes generated
and temporary content, and replaces existing outputs. Ensure `./dist` exists
before running the command.

```sh
dtar ./my-project \
  --output ./dist/my-project.tar.gz \
  --checksum \
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
