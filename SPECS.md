# Specification: Deterministic .tar.gz Compressor in Rust

## Objective

Build a command-line tool or library module in Rust that compresses one or more
files or directories into a .tar.gz archive. The output file must be 100%
deterministic (reproducible). Running this utility on the same input paths,
contents, and normalized executable status must always yield a byte-for-byte
identical output file with an identical cryptographic checksum (e.g., SHA-256),
regardless of the host operating system, filesystem state, user context, or
execution timestamp.

## Core Dependencies

The implementation must use the following standard Rust crates:
- tar (Recommended version: 0.4) – For archive formatting.
- flate2 (Recommended version: 1.0) – For Gzip DEFLATE compression.

## Strict Determinism Requirements

### Directory Traversal and File Ordering

- Problem: Filesystem APIs (std::fs::read_dir) return entries in an unpredictable order dictated by the OS kernel and disk layouts.
- Requirement: The agent must recursively collect all target files and directories into an in-memory collection and sort them strictly by their relative archive path (lexicographically) before writing them to the tar payload stream.

### Path Normalization

- Problem: Windows uses backslashes (\) for file paths, whereas Unix-like systems use forward slashes (/).
- Requirement: The internal file paths stored inside the .tar headers must strictly use forward slashes (/) as separators, ensuring cross-platform path consistency.

### Tar Header Sanitization

The agent must construct a fresh GNU or USTAR tar header for every entry and overwrite all volatile metadata field variables with static, hardcoded constants:
- Modification Time (mtime): Must be hardcoded to 0 (representing the Unix Epoch: January 1, 1970, 00:00:00 UTC).
- User ID (uid) & Group ID (gid): Must be hardcoded to 0.
- User Name & Group Name: Must be cleared or left empty.
- File Permissions (mode): Directories must use 0o755. Regular files must use
  0o755 if the source has any executable bit set, and 0o644 otherwise. Ignore
  all other source permission bits. On platforms whose metadata does not expose
  Unix executable bits, use 0o644 for regular files.

###  Gzip Compression Layer

- Problem: The Gzip format standard specifies a 4-byte timestamp header field that reflects the creation time by default.
- Requirement: The flate2 encoder must be configured to suppress or zero out the Gzip header timestamp (equivalent to gzip -n). Note: The default flate2 GzEncoder configuration satisfies this by omitting the metadata header unless explicitly added.
- Compression Level: The compression configuration must be locked to a specific profile (e.g., Compression::best() or Compression::new(9)) to ensure the DEFLATE bitstream mapping remains uniform across iterations.

### Implementation Steps for the Agent

1. Initialize Output Stream: Create a temporary file in the destination directory. Do not write directly to or truncate the requested output path.
2. Layer the Encoders: Instantiat a flate2::write::GzEncoder linked to the file handle, and wrap it into a tar::Builder.
3. Collect Entries: For each file or directory source, recursively collect supported entries. A single directory source uses paths relative to its contents; otherwise, put each source under its basename. Convert archive paths to forward slashes.
4. Sort Entries: Sort the list of collected entries lexicographically based on their normalized relative path.
5. Write with Clean Headers: Iteratively process each entry:
	1. Initialize a clean tar::Header::new_gnu().
	2. Apply static configurations (mtime = 0, uid = 0, gid = 0, fixed mode).
	3. Set the exact file size payload attribute.
	4. Append the structured header and file data payload to the archive stream.
6. Flush and Finalize: Explicitly invoke the teardown methods for the tar container structure (archive.into_inner()?) and flush the compression stream buffer completely (encoder.finish()?) to guarantee data integrity.
7. Publish Output: Flush and synchronize the completed temporary archive, calculate its checksum, then publish it at the requested output path. Publish without replacing an existing file by default; `--force` must atomically replace it.

### Output Safety and Failure Handling

The resolved output path must be outside every directory input and must not be
the path of any file input. If either condition is violated, fail before
creating or modifying any output.
The archive must be written to a temporary file in the destination directory and published only after writing, tar/gzip finalization, flushing, synchronization, and checksum calculation all succeed. If any of these operations fails before publication, the requested output path must remain unchanged: an existing archive must be preserved, and a first-time run must not leave a partial archive there. On handled errors, discard the temporary file. The temporary file must be on the same filesystem as the destination so publication can be atomic.

### Source Changes During Compression

Collect and sort the archive plan, including each regular file's size and
whether it is executable, before writing begins. Entries added after planning
are not included. When each planned regular file is opened for writing, verify
it is still a regular file with the planned size; if it cannot be opened or
this check fails, abort without publishing the temporary archive. This check
does not provide a filesystem snapshot: the program does not lock source files,
and same-size content changes are not detected. File contents are read during
compression, so concurrent modifications may result in the bytes observed
during the read rather than a consistent point-in-time copy. A permission change
after planning does not change the mode already recorded in the archive plan.

## Command line

The command syntax is `dtar [OPTIONS] <SOURCE>...`. Require at least one source
path; each path may name a regular file or directory. With one directory,
archive its contents without a top-level directory entry to preserve the
existing layout. Store a single file under its basename. With multiple inputs,
store each under its basename, including the basename and descendants for
directories. Reject unsupported entries and inputs whose archive paths
collide. Normalize all resulting archive paths and sort entries
deterministically. With one source, default to an archive beside it named
`<source-name>.tar.gz`. Require `--output` for multiple sources. Apply
`--exclude` to normalized archive-relative paths; excluded directories and
their descendants are omitted.

The CLI must provide help and version flags, a progress bar for compression,
and an archive checksum on successful completion. The optional `--checksum`
flag must write a `SHA256SUMS` manifest beside the archive containing the
archive's lowercase hexadecimal SHA-256 and its filename in standard
`sha256sum` format. Do not write the manifest unless archive compression and
publication succeed. If the manifest already exists, require `--force` before
replacing it. The output archive may not itself be named `SHA256SUMS` when the
option is enabled.

### Completion Statistics

After a successful compression, the CLI must print the elapsed end-to-end time,
the number of regular files and directories archived, the total uncompressed
file size, the final archive size, and the archive's SHA-256 checksum unless
quiet mode is enabled.

Statistics must describe only a successfully published archive. Counts include
the entries actually stored (a single directory input's root is omitted);
source size is the total byte size of the archived regular files; archive size
is the final `.tar.gz` size in bytes; and the checksum is the lowercase
hexadecimal SHA-256 of the complete archive.
Elapsed time starts with input validation and ends after the archive is
persisted, including planning, writing, finalization, hashing, and publication,
but excluding CLI output.

### Failure Behavior

Any validation, source-reading, archive-writing, finalization, hashing, or
publication failure must cause a non-zero CLI exit status and an error on
stderr, even in quiet mode. Errors must retain context identifying the failed
operation and relevant path or input when applicable. Do not report completion
statistics or a successful archive path on failure. Apply the output safety
requirements above so failures before publication do not leave a partial
archive at the requested destination.

### Quiet Mode

The `--quiet`/`-q` option must suppress the progress bar and successful
completion output, including the dry-run tree. Errors must still be reported.
With `--dry-run --checksum`, validate and report the planned manifest path
without creating or replacing it.

### Excluding Entries

The CLI must accept repeatable `--exclude PATTERN` options. Patterns must be
valid globs matched against normalized archive-relative paths using `/`
separators. A pattern without `/` matches a path component at any depth; a
pattern containing `/` is relative to the archive root. `*` and `?` do not
match `/`; `**` may match across directory separators. An excluded directory
and its descendants must be omitted; there are no implicit exclusions.
Invalid patterns must produce an error rather than being ignored.

The CLI must also accept `--exclude-os-artifacts`, which adds these patterns to
the user-supplied exclusions: `.DS_Store`, `._*`, `.Spotlight-V100`,
`.fseventsd`, `.Trashes`, `.TemporaryItems`, `Thumbs.db`, `ehthumbs.db`,
`desktop.ini`, `$RECYCLE.BIN`, `System Volume Information`, `.directory`,
`.Trash-*`, and `lost+found`. These slashless patterns match at any depth under
the same glob rules as `--exclude`; matching directories and their descendants
are omitted. The option is opt-in, and its patterns must be applied identically
to archive planning, dry-run previews, and compression.

### Dry Run

The `--dry-run` option must validate all source paths, the destination, and
exclusions, then print a tree of the entries that would be archived in
deterministic order. It must not create, truncate, or replace the output
archive. Exclusions must apply to the preview. Quiet mode suppresses the tree.

## Verification Criteria (Definition of Done)

The agent must provide a verification test (such as a local integration test) demonstrating that:
1. Compressing an arbitrary folder twice results in identical files.
2. Manually changing a local file's modification time on disk (touch command) and rerunning the compressor still yields an identical SHA-256 hash output.
3. A dry run prints the included archive tree, respects exclusions, and does not
   create or replace the output file.
4. A failure after writing has begun does not publish a partial archive, and
   preserves an existing destination even when replacement was requested.
5. Multiple file and directory inputs are archived under their basenames,
   single-directory input retains its existing layout, and dry-run matches the
   resulting archive tree.