# Repository guidance

## Build, test, and lint

- Build the optimized CLI: `cargo build --release`
- Run all tests: `cargo test`
- Run the determinism integration tests: `cargo test --test determinism`
- Run one integration test, for example: `cargo test --test determinism repeated_archives_ignore_mtime_and_filesystem_order`
- Check formatting: `cargo fmt --check`
- Run Clippy with warnings treated as errors: `cargo clippy --all-targets -- -D warnings`

## CI workflow

`.github/workflows/ci.yml` skips pushes and pull requests when all changed files
match its documentation-only `paths-ignore` patterns (`**.md` and `docs/**`).
Mixed documentation and code changes still run CI. Use the workflow's
`workflow_dispatch` trigger in GitHub Actions to run CI manually for a
documentation-only change. Keep both event filters aligned if documentation
uses additional paths or file extensions.

To publish a release, move the release entries from `Unreleased` in
`CHANGELOG.md` under the new version/date heading, then tag and push the release
commit using a stable `vMAJOR.MINOR.PATCH` tag:

```sh
git tag v0.0.1
git push origin v0.0.1
```

CI verifies and builds all six platform targets before creating the GitHub
Release with target-specific packages attached (`.tar.gz` for Linux/macOS and
`.zip` for Windows).

## Architecture

`src/main.rs` defines the `clap` CLI (`dtar <SOURCE>`, with output, overwrite,
and quiet options), drives an `indicatif` progress bar, calls the library, and
prints its completion statistics and SHA-256.

`src/lib.rs` implements the reusable compressor. It canonicalizes and validates
paths, recursively collects regular files and directories, normalizes and sorts
archive paths, writes sanitized tar headers through gzip to a temporary file,
finalizes and hashes that file, then atomically persists it and returns an
`ArchiveStats` report. The progress-aware library entry point is shared by the
CLI and the no-op-progress API wrapper.

`tests/determinism.rs` exercises reproducibility, metadata and entry ordering,
and protection of an existing output. `SPECS.md` defines the required archive
behavior; `README.md` documents the supported inputs and CLI.

## Project-specific invariants

- Keep the root `SPECS.md` authoritative: whenever the program's requirements or
  specified behavior change, update `SPECS.md` in the same change.
- Keep the root `README.md` usage guidance current: whenever a change affects
  how users invoke or interact with the program, update `README.md` in the same
  change.
- Maintain a root-level `CHANGELOG.md` that lists user-visible changes. Update
  it alongside each change, grouping entries under an appropriate version or
  date heading.
- Write commit subjects in Conventional Commit format (`type(scope): summary`)
  and end each subject with one relevant emoji, for example
  `fix(archive): preserve existing output 🛡️`.
- Archive bytes must not depend on filesystem enumeration order, host paths,
  file modification times, ownership, or permissions. Collect entries before
  writing, sort by normalized relative archive path, and use `/` separators.
- Keep tar metadata fixed: mtime, uid, and gid are zero; owner/group names are
  empty; regular files use mode `0o644` and directories `0o755`. Keep the gzip
  timestamp at zero, OS byte at `255`, the Rust `flate2` backend, and the
  fixed best-compression profile. If any of these change, update the metadata
  and byte-for-byte determinism tests deliberately.
- The supported source entries are regular files and directories only.
  Symbolic links, special entries, and non-UTF-8 archive paths produce errors;
  do not silently follow, skip, or rewrite them. The output must remain outside
  the source tree.
- Write to a temporary file in the destination directory and only publish a
  fully finalized, hashed archive. Preserve an existing destination unless
  overwrite was explicitly requested.
- Both Rust targets forbid unsafe code. Preserve contextual errors through
  `anyhow` so CLI failures identify the operation and path that failed.
