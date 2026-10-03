use std::{
    fs::{self, File},
    io::Read,
    process::Command,
    time::{Duration, SystemTime},
};

use flate2::read::GzDecoder;
use tar::Archive;
use tempfile::tempdir;

#[test]
fn repeated_archives_ignore_mtime_and_filesystem_order() {
    let directory = tempdir().unwrap();
    let source = directory.path().join("input");
    fs::create_dir_all(source.join("z-dir")).unwrap();
    fs::create_dir_all(source.join("a-dir")).unwrap();
    fs::write(source.join("z-dir/z.txt"), b"last").unwrap();
    let file = source.join("a-dir/a.txt");
    fs::write(&file, b"first").unwrap();
    let first = directory.path().join("first.tar.gz");
    let second = directory.path().join("second.tar.gz");

    let first_stats = dtar::compress_directory(&source, &first, false).unwrap();
    let old = SystemTime::UNIX_EPOCH + Duration::from_secs(42);
    File::options()
        .write(true)
        .open(&file)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(old))
        .unwrap();
    let second_stats = dtar::compress_directory(&source, &second, false).unwrap();

    assert_eq!(first_stats.sha256, second_stats.sha256);
    assert_eq!(fs::read(first).unwrap(), fs::read(second).unwrap());
}

#[test]
fn archive_entries_have_normalized_order_and_metadata() {
    let directory = tempdir().unwrap();
    let source = directory.path().join("input");
    fs::create_dir_all(source.join("z-dir")).unwrap();
    fs::create_dir_all(source.join("a-dir")).unwrap();
    fs::write(source.join("z-dir/z.txt"), b"last").unwrap();
    fs::write(source.join("a-dir/a.txt"), b"first").unwrap();
    let output = directory.path().join("result.tar.gz");
    let stats = dtar::compress_directory(&source, &output, false).unwrap();
    assert_eq!(stats.files, 2);
    assert_eq!(stats.directories, 2);
    assert_eq!(stats.source_bytes, 9);

    let bytes = fs::read(&output).unwrap();
    assert_eq!(stats.archive_bytes, bytes.len() as u64);
    assert!(!stats.sha256.is_empty());
    assert_eq!(&bytes[4..8], &[0, 0, 0, 0]);
    assert_eq!(bytes[9], 255);

    let decoder = GzDecoder::new(File::open(output).unwrap());
    let mut archive = Archive::new(decoder);
    let entries = archive
        .entries()
        .unwrap()
        .map(|entry| {
            let mut entry = entry.unwrap();
            let path = entry.path().unwrap().to_string_lossy().into_owned();
            let header = entry.header();
            assert_eq!(header.mtime().unwrap(), 0);
            assert_eq!(header.uid().unwrap(), 0);
            assert_eq!(header.gid().unwrap(), 0);
            assert_eq!(header.username().unwrap(), Some(""));
            assert_eq!(header.groupname().unwrap(), Some(""));
            let expected_mode = if path.ends_with('/') || path.ends_with("-dir") {
                0o755
            } else {
                0o644
            };
            assert_eq!(header.mode().unwrap(), expected_mode);
            let mut payload = Vec::new();
            entry.read_to_end(&mut payload).unwrap();
            (path, payload)
        })
        .collect::<Vec<_>>();

    assert_eq!(
        entries
            .iter()
            .map(|(path, _)| path.as_str())
            .collect::<Vec<_>>(),
        ["a-dir", "a-dir/a.txt", "z-dir", "z-dir/z.txt"]
    );
}

#[test]
fn existing_output_is_preserved_without_force() {
    let directory = tempdir().unwrap();
    let source = directory.path().join("input");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("file"), b"content").unwrap();
    let output = directory.path().join("result.tar.gz");
    fs::write(&output, b"keep me").unwrap();

    let error = dtar::compress_directory(&source, &output, false).unwrap_err();
    assert!(error.to_string().contains("already exists"));
    assert_eq!(fs::read(output).unwrap(), b"keep me");
}

#[test]
fn cli_prints_compression_statistics() {
    let directory = tempdir().unwrap();
    let source = directory.path().join("input");
    fs::create_dir_all(source.join("nested")).unwrap();
    fs::write(source.join("nested/file.txt"), b"hello").unwrap();
    fs::write(source.join("nested/ignored.tmp"), b"skip").unwrap();
    let output_path = directory.path().join("result.tar.gz");

    let output = Command::new(env!("CARGO_BIN_EXE_dtar"))
        .args(["--output"])
        .arg(output_path)
        .args(["--exclude", "*.tmp"])
        .arg(source)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "dtar failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Files: 1 | Directories: 1\nSource size: 5 bytes | Archive size:"));
    assert!(stdout.contains("Elapsed:"));
    assert!(stdout.contains("SHA-256:"));
}

#[test]
fn quiet_flag_suppresses_progress_and_success_summary() {
    let directory = tempdir().unwrap();
    let source = directory.path().join("input");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("file.txt"), b"hello").unwrap();
    let output_path = directory.path().join("result.tar.gz");

    let output = Command::new(env!("CARGO_BIN_EXE_dtar"))
        .args(["--quiet", "--output"])
        .arg(&output_path)
        .arg(source)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "dtar failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    assert!(output_path.is_file());
}

#[test]
fn dry_run_prints_excluded_archive_tree_without_creating_output() {
    let directory = tempdir().unwrap();
    let source = directory.path().join("input");
    fs::create_dir_all(source.join("nested")).unwrap();
    fs::write(source.join("nested/keep.txt"), b"keep").unwrap();
    fs::write(source.join("nested/skip.tmp"), b"skip").unwrap();
    let output_path = directory.path().join("result.tar.gz");

    let output = Command::new(env!("CARGO_BIN_EXE_dtar"))
        .args(["--dry-run", "--output"])
        .arg(&output_path)
        .args(["--exclude", "*.tmp"])
        .arg(&source)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "dtar failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let resolved_output = directory
        .path()
        .canonicalize()
        .unwrap()
        .join("result.tar.gz");
    assert!(stdout.contains("Dry run: no archive will be created."));
    assert!(stdout.contains(&format!("Output: {}", resolved_output.display())));
    assert!(stdout.contains("input/\n`-- nested/\n    `-- keep.txt"));
    assert!(!stdout.contains("skip.tmp"));
    assert!(!output_path.exists());

    fs::write(&output_path, b"existing archive").unwrap();
    let overwrite_preview = Command::new(env!("CARGO_BIN_EXE_dtar"))
        .args(["--dry-run", "--force", "--output"])
        .arg(&output_path)
        .arg(&source)
        .output()
        .unwrap();
    assert!(
        overwrite_preview.status.success(),
        "dtar failed: {}",
        String::from_utf8_lossy(&overwrite_preview.stderr)
    );
    assert_eq!(fs::read(output_path).unwrap(), b"existing archive");
}

#[test]
fn exclude_patterns_skip_files_and_prune_directories() {
    let directory = tempdir().unwrap();
    let source = directory.path().join("input");
    fs::create_dir_all(source.join("build/cache")).unwrap();
    fs::create_dir_all(source.join("src")).unwrap();
    fs::write(source.join("build/cache/data.bin"), b"ignored").unwrap();
    fs::write(source.join("src/discard.tmp"), b"ignored").unwrap();
    fs::write(source.join("src/keep.rs"), b"keep").unwrap();
    let output = directory.path().join("filtered.tar.gz");
    let exclude_patterns = vec!["build".to_owned(), "*.tmp".to_owned()];

    let stats =
        dtar::compress_directory_with_excludes(&source, &output, false, &exclude_patterns).unwrap();

    assert_eq!(stats.files, 1);
    assert_eq!(stats.directories, 1);
    assert_eq!(stats.source_bytes, 4);

    let decoder = GzDecoder::new(File::open(output).unwrap());
    let mut archive = Archive::new(decoder);
    let paths = archive
        .entries()
        .unwrap()
        .map(|entry| {
            entry
                .unwrap()
                .path()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect::<Vec<_>>();
    assert_eq!(paths, ["src", "src/keep.rs"]);

    let invalid_pattern = vec!["[".to_owned()];
    let error = dtar::compress_directory_with_excludes(
        &source,
        directory.path().join("invalid.tar.gz"),
        false,
        &invalid_pattern,
    )
    .unwrap_err();
    assert!(error.to_string().contains("invalid exclude pattern"));
}
