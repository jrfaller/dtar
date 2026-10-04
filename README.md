# dtar

`dtar` turns files and directories into `.tar.gz` archives with normalized
metadata and compression, so the same inputs produce the same archive bytes.

## Why aren't `.tar.gz` files reproducible by default?

A `.tar.gz` file is two formats layered together: a **tar archive** containing
the files, wrapped in a **gzip stream** that compresses the tar bytes. The files
you extract can be identical while the archive's bytes differ, because both
layers can record choices that were incidental to creating the archive.

The tar layer stores more than file contents. Each entry has a path and
metadata, including modification time, permissions, and ownership. Directory
traversal order can vary between filesystems or runs, and tools can choose
different path layouts or tar header formats. As a result, two archives of the
same files may have different entry order, timestamps, owners, modes, or header
bytes.

Gzip adds another source of variation. Its header can include a timestamp,
original filename, and platform marker. Even with those fields normalized,
different compression levels, strategies, or compressor implementations and
versions can encode the same tar data into different valid DEFLATE byte
streams. Reproducibility means matching the complete bytes—not merely being
able to extract the same files—so a checksum changes if any of these choices
change.

## Why not just run `strip_nondeterminism`?

Tools such as `strip_nondeterminism` can normalize known sources of variation
in archive formats they support, and they are useful in packaging workflows.
But a post-processing command is not a universal portability guarantee. Its
coverage and behavior depend on the archive format and tool version; it cannot
decide every project's intended path layout, file selection, or permission
policy. Nor does normalizing metadata alone ensure that different compressors
or versions emit the same DEFLATE stream. A portable reproducible build must
control the archive contents and ordering, the metadata policy, and the
compression settings and implementation—not just remove a few timestamps
afterward.

## How `dtar` makes archives reproducible

`dtar` controls those choices while creating the archive: it sorts entries by
normalized archive path, fixes tar timestamps and ownership, and normalizes
file modes: executable regular files and directories use `0o755`, while other
regular files use `0o644`. On filesystems that do not expose executable status,
regular files use `0o644`. It also normalizes gzip header fields and uses the
Rust DEFLATE backend with a fixed best-compression profile. Given the same
inputs, `dtar` version, and options, repeated runs produce the same archive
bytes and SHA-256 checksum.

## When is `dtar` useful?

- **Builds:** with the same files, paths, and `dtar` settings, the archive
  bytes stay identical even if timestamps or directory listing order differ.
  A build cache can then reuse the archive instead of treating those
  irrelevant changes as new output.
- **Reproducing a release:** rebuild an archive from the same files and paths,
  then compare its SHA-256 with the project's trusted published checksum. A
  match confirms the archive bytes are identical; you must trust the source of
  the published checksum.
- **Storage and version control:** stable hashes help identify and deduplicate
  unchanged backups, while rerunning an archive command avoids timestamp-only
  changes to generated archives tracked in Git.

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
