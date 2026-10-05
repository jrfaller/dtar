# Changelog

## Unreleased

### Changed

- Upgrade GitHub Actions checkout and artifact actions to their latest major
  versions.
- Use deterministic PAX extended headers for archive paths and metadata,
  followed by USTAR entry headers.

## 0.2.0 - 2026-10-04

### Changed

- Add a concise `dtar` overview, explain why `.tar.gz` archives vary by default
  and why post-processing alone does not guarantee portable reproducibility,
  and group the tool's use cases in a dedicated section.
- Match exclude patterns with Gitignore-style path semantics: slashless
  patterns match at any depth, while `*` no longer crosses `/`.

### Added

- Add the opt-in `--exclude-os-artifacts` preset for common OS metadata and
  system folders.
- Preserve executable status in archive modes where the source filesystem
  exposes it.

## 0.1.0 - 2026-10-04

### Changed

- Define CLI failure reporting and completion-statistic semantics in the spec.
- Document behavior when source files change during compression.
- Specify that output paths must not overlap source inputs.
- Require `--output` when archiving multiple source paths.
- Specify and verify that failed archive writes do not publish partial output.
- License the project under the MIT License.
- Print compression duration, entry counts, source and archive sizes, and SHA-256.
- Make `--quiet` suppress the successful-completion summary as well as progress.
- Explain deterministic archive use cases in the README.
- Document the benefit of storing generated archives in Git.
- Expand README usage documentation with an example for every CLI argument and
  option.

### Added

- Write an optional `SHA256SUMS` manifest beside the archive with `--checksum`.
- Accept multiple file and directory inputs as positional source paths.
- Publish GitHub Releases with target-specific binary packages when a stable
  `vMAJOR.MINOR.PATCH` tag is pushed.
- Attach a `SHA256SUMS` manifest for release packages.
- Support repeatable glob patterns for excluding files and directories from
  archives.
- Preview the archive entry tree with `--dry-run` without creating an archive.
