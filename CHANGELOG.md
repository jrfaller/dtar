# Changelog

## Unreleased

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

- Accept multiple file and directory inputs as positional source paths.
- Publish GitHub Releases with target-specific binary packages when a stable
  `vMAJOR.MINOR.PATCH` tag is pushed.
- Attach a `SHA256SUMS` manifest for release packages.
- Support repeatable glob patterns for excluding files and directories from
  archives.
- Preview the archive entry tree with `--dry-run` without creating an archive.
