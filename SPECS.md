# Specification: Deterministic .tar.gz Compressor in Rust

## Objective

Build a command-line tool or library module in Rust that compresses a target directory into a .tar.gz archive. The output file must be 100% deterministic (reproducible). Running this utility on the same input directory structure must always yield a byte-for-byte identical output file with an identical cryptographic checksum (e.g., SHA-256), regardless of the host operating system, filesystem state, user context, or execution timestamp.

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
- File Permissions (mode): Must be normalized to a fixed permission bitmask (e.g., 0o644 for regular files, 0o755 for directories or executable files). Do not inherit permissions from the host filesystem.

###  Gzip Compression Layer

- Problem: The Gzip format standard specifies a 4-byte timestamp header field that reflects the creation time by default.
- Requirement: The flate2 encoder must be configured to suppress or zero out the Gzip header timestamp (equivalent to gzip -n). Note: The default flate2 GzEncoder configuration satisfies this by omitting the metadata header unless explicitly added.
- Compression Level: The compression configuration must be locked to a specific profile (e.g., Compression::best() or Compression::new(9)) to ensure the DEFLATE bitstream mapping remains uniform across iterations.

### Implementation Steps for the Agent

1. Initialize Output Stream: Create a temporary file in the destination directory. Do not write directly to or truncate the requested output path.
2. Layer the Encoders: Instantiat a flate2::write::GzEncoder linked to the file handle, and wrap it into a tar::Builder.
3. Collect Entries: Recursively traverse the source directory. Strip the root prefix from each file to compute its relative archive path. Convert backslashes to forward slashes.
4. Sort Entries: Sort the list of collected entries lexicographically based on their normalized relative path.
5. Write with Clean Headers: Iteratively process each entry:
	1. Initialize a clean tar::Header::new_gnu().
	2. Apply static configurations (mtime = 0, uid = 0, gid = 0, fixed mode).
	3. Set the exact file size payload attribute.
	4. Append the structured header and file data payload to the archive stream.
6. Flush and Finalize: Explicitly invoke the teardown methods for the tar container structure (archive.into_inner()?) and flush the compression stream buffer completely (encoder.finish()?) to guarantee data integrity.
7. Publish Output: Flush and synchronize the completed temporary archive, calculate its checksum, then publish it at the requested output path. Publish without replacing an existing file by default; `--force` must atomically replace it.

### Output Safety and Failure Handling

The resolved output path must be outside the source directory. If it is inside
or equal to the source directory, fail before creating or modifying any output.
The archive must be written to a temporary file in the destination directory and published only after writing, tar/gzip finalization, flushing, synchronization, and checksum calculation all succeed. If any of these operations fails before publication, the requested output path must remain unchanged: an existing archive must be preserved, and a first-time run must not leave a partial archive there. On handled errors, discard the temporary file. The temporary file must be on the same filesystem as the destination so publication can be atomic.

## Command line

The `dtar` command must have the flags an option expected for a compression command, as well as a help and version flags. Also, it should be user-friendly: gracefully warn on errors, use a progress bar for the compression task, announce the archive checksum on completion.

### Completion Statistics

After a successful compression, the CLI must print the elapsed end-to-end time,
the number of regular files and directories archived (excluding the source
root), the total uncompressed file size, the final archive size, and the
archive's SHA-256 checksum unless quiet mode is enabled.

### Quiet Mode

The `--quiet`/`-q` option must suppress the progress bar and successful
completion output, including the dry-run tree. Errors must still be reported.

### Excluding Entries

The CLI must accept repeatable `--exclude PATTERN` options. Patterns must be
valid globs matched against normalized, source-relative paths using `/`
separators. Wildcards may match across directory separators. An excluded
directory and its descendants must be omitted; there are no implicit
exclusions. Invalid patterns must produce an error rather than being ignored.

### Dry Run

The `--dry-run` option must validate the source, destination, and exclusions,
then print a tree of the entries that would be archived in deterministic order.
It must not create, truncate, or replace the output archive. Exclusions must
apply to the preview. Quiet mode suppresses the tree.

## Verification Criteria (Definition of Done)

The agent must provide a verification test (such as a local integration test) demonstrating that:
1. Compressing an arbitrary folder twice results in identical files.
2. Manually changing a local file's modification time on disk (touch command) and rerunning the compressor still yields an identical SHA-256 hash output.
3. A dry run prints the included archive tree, respects exclusions, and does not
   create or replace the output file.
4. A failure after writing has begun does not publish a partial archive, and
   preserves an existing destination even when replacement was requested.