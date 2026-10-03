# Changelog

## Unreleased

### Changed

- License the project under the MIT License.
- Print compression duration, entry counts, source and archive sizes, and SHA-256.

### Added

- Publish GitHub Releases with target-specific binary packages when a stable
  `vMAJOR.MINOR.PATCH` tag is pushed.
- Attach a `SHA256SUMS` manifest for release packages.
- Support repeatable glob patterns for excluding files and directories from
  archives.
