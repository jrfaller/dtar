# dtar

`dtar` creates reproducible `.tar.gz` archives from directories. File and
directory entries are sorted by normalized relative path; tar timestamps,
owners, names, and permissions are fixed, and the gzip timestamp and operating
system fields are normalized. Compression uses the Rust DEFLATE backend and a
fixed best-compression profile.

The tool archives regular files and directories. Symbolic links and other
special filesystem entries are rejected rather than followed. Archive paths
must be valid UTF-8. Output archives must be outside the source directory.

## Usage

```text
dtar [OPTIONS] <SOURCE>
```

By default, the archive is created next to the source directory as
`<source-name>.tar.gz`. Use `-o`/`--output` to choose a path, `-f`/`--force` to
replace an existing archive, and `-q`/`--quiet` to hide the progress bar. On
success, `dtar` prints the archive's SHA-256 checksum. `--help` and `--version`
are provided by the CLI.

## Build and test

```sh
cargo build --release
cargo test
```
