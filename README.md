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
replace an existing archive, repeat `-e`/`--exclude` to filter paths with glob
patterns, and use `-q`/`--quiet` to hide the progress bar and success summary.
Errors are still displayed in quiet mode. Patterns match source-relative paths
with `/` separators; `*` can match across directories. Matching a directory
excludes its whole subtree. No paths are excluded by default. For example:

```sh
dtar ./my-project --exclude .git --exclude target --exclude '*.tmp'
```

On success, `dtar` prints the file and directory counts, source and archive
sizes, elapsed time, and the archive's SHA-256 checksum unless quiet mode is
enabled. `--help` and `--version` are provided by the CLI.

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
