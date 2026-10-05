use std::{
    fs::{self, File},
    io::Read,
    process::Command,
    time::{Duration, SystemTime},
};

use flate2::read::GzDecoder;
use sha2::{Digest, Sha256};
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
fn fixed_fixture_archive_checksum() {
    let directory = tempdir().unwrap();
    let source = directory.path().join("input");
    fs::create_dir_all(source.join("nested")).unwrap();
    fs::write(source.join("alpha.txt"), b"dtar cross-target fixture\n").unwrap();
    fs::write(source.join("nested/omega.txt"), b"stable bytes\n").unwrap();
    let output = directory.path().join("result.tar.gz");

    let stats = dtar::compress_directory(&source, &output, false).unwrap();
    let bytes = fs::read(output).unwrap();
    let checksum = format!("{:x}", Sha256::digest(bytes));
    assert_eq!(stats.sha256, checksum);

    if let Some(checksum_path) = std::env::var_os("DTAR_CROSS_TARGET_HASH_FILE") {
        fs::write(checksum_path, format!("{checksum}\n")).unwrap();
    }
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
            let pax_metadata = entry
                .pax_extensions()
                .unwrap()
                .expect("each entry should have PAX metadata")
                .map(|extension| {
                    let extension = extension.unwrap();
                    (
                        extension.key().unwrap().to_owned(),
                        extension.value().unwrap().to_owned(),
                    )
                })
                .collect::<Vec<_>>();
            assert_eq!(
                pax_metadata,
                [
                    ("path".to_owned(), path.clone()),
                    ("mtime".to_owned(), "0".to_owned()),
                    ("uid".to_owned(), "0".to_owned()),
                    ("gid".to_owned(), "0".to_owned()),
                    ("uname".to_owned(), String::new()),
                    ("gname".to_owned(), String::new()),
                ]
            );
            let header = entry.header();
            assert_eq!(&header.as_bytes()[257..263], b"ustar\0");
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
fn pax_headers_preserve_long_archive_paths() {
    let directory = tempdir().unwrap();
    let source = directory.path().join("input");
    let mut nested = source.clone();
    for _ in 0..4 {
        nested = nested.join("a".repeat(80));
    }
    fs::create_dir_all(&nested).unwrap();
    let file = nested.join("file.txt");
    fs::write(&file, b"long path").unwrap();
    let output = directory.path().join("result.tar.gz");

    dtar::compress_directory(&source, &output, false).unwrap();

    let expected_path = format!(
        "{}/{}/{}/{}/file.txt",
        "a".repeat(80),
        "a".repeat(80),
        "a".repeat(80),
        "a".repeat(80)
    );
    assert!(expected_path.len() > 255);

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
    assert!(paths.contains(&expected_path));
}

#[cfg(unix)]
#[test]
fn executable_bit_is_preserved_in_archive_mode() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempdir().unwrap();
    let source = directory.path().join("input");
    fs::create_dir(&source).unwrap();
    let executable = source.join("run.sh");
    fs::write(&executable, b"#!/bin/sh\n").unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
    let output = directory.path().join("result.tar.gz");

    dtar::compress_directory(&source, &output, false).unwrap();

    let decoder = GzDecoder::new(File::open(output).unwrap());
    let mut archive = Archive::new(decoder);
    let entry = archive.entries().unwrap().next().unwrap().unwrap();
    assert_eq!(entry.path().unwrap().to_string_lossy(), "run.sh");
    assert_eq!(entry.header().mode().unwrap(), 0o755);
}

#[test]
fn multiple_sources_archive_under_their_basenames() {
    let directory = tempdir().unwrap();
    let file_dir = directory.path().join("code");
    let assets = directory.path().join("assets");
    fs::create_dir_all(&file_dir).unwrap();
    fs::create_dir_all(assets.join("icons")).unwrap();
    let main_file = file_dir.join("main.rs");
    fs::write(&main_file, b"main").unwrap();
    fs::write(directory.path().join("README.md"), b"readme").unwrap();
    fs::write(assets.join("icons/app.png"), b"image").unwrap();
    let sources = vec![main_file, directory.path().join("README.md"), assets];
    let output = directory.path().join("multiple.tar.gz");

    let stats = dtar::compress_sources(&sources, &output, false).unwrap();

    assert_eq!(stats.files, 3);
    assert_eq!(stats.directories, 2);
    assert_eq!(stats.source_bytes, 15);
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
    assert_eq!(
        paths,
        [
            "README.md",
            "assets",
            "assets/icons",
            "assets/icons/app.png",
            "main.rs"
        ]
    );
}

#[test]
fn single_directory_source_keeps_contents_only_layout() {
    let directory = tempdir().unwrap();
    let source = directory.path().join("project");
    fs::create_dir_all(source.join("src")).unwrap();
    fs::write(source.join("src/main.rs"), b"main").unwrap();
    let output = directory.path().join("single.tar.gz");

    dtar::compress_sources(std::slice::from_ref(&source), &output, false).unwrap();

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
    assert_eq!(paths, ["src", "src/main.rs"]);
}

#[test]
fn exclusions_apply_to_normalized_paths_from_multiple_sources() {
    let directory = tempdir().unwrap();
    let assets = directory.path().join("assets");
    fs::create_dir(&assets).unwrap();
    fs::write(assets.join("keep.png"), b"keep").unwrap();
    fs::write(assets.join("skip.tmp"), b"skip").unwrap();
    let readme = directory.path().join("README.md");
    fs::write(&readme, b"readme").unwrap();
    let sources = vec![assets, readme];
    let output = directory.path().join("filtered.tar.gz");
    let excludes = vec!["assets/*.tmp".to_owned()];

    let stats = dtar::compress_sources_with_excludes(&sources, &output, false, &excludes).unwrap();

    assert_eq!(stats.files, 2);
    assert_eq!(stats.directories, 1);
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
    assert_eq!(paths, ["README.md", "assets", "assets/keep.png"]);
}

#[test]
fn duplicate_source_basenames_are_rejected() {
    let directory = tempdir().unwrap();
    let first_dir = directory.path().join("first");
    let second_dir = directory.path().join("second");
    fs::create_dir_all(&first_dir).unwrap();
    fs::create_dir_all(&second_dir).unwrap();
    let first = first_dir.join("same.txt");
    let second = second_dir.join("same.txt");
    fs::write(&first, b"first").unwrap();
    fs::write(&second, b"second").unwrap();
    let sources = vec![first, second];
    let excludes = vec!["same.txt".to_owned()];

    let error = dtar::plan_sources(
        &sources,
        directory.path().join("duplicate.tar.gz"),
        false,
        &excludes,
    )
    .err()
    .expect("duplicate archive paths were accepted");
    assert!(error.to_string().contains("same archive path"));
}

#[test]
fn output_cannot_replace_a_file_source_even_with_force() {
    let directory = tempdir().unwrap();
    let source = directory.path().join("source.txt");
    fs::write(&source, b"preserve me").unwrap();

    let error = dtar::plan_sources(std::slice::from_ref(&source), &source, true, &[])
        .err()
        .expect("output was allowed to replace a source file");

    assert!(error.to_string().contains("cannot replace source file"));
    assert_eq!(fs::read(source).unwrap(), b"preserve me");
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
fn failed_overwrite_preserves_output_and_cleans_temporary_archive() {
    let directory = tempdir().unwrap();
    let source = directory.path().join("input");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("a.txt"), b"first").unwrap();
    let second_file = source.join("b.txt");
    fs::write(&second_file, b"second").unwrap();
    let output = directory.path().join("result.tar.gz");
    fs::write(&output, b"existing archive").unwrap();

    let error = dtar::compress_directory_with_excludes_and_progress(
        &source,
        &output,
        true,
        &[],
        |completed, _| {
            if completed == 1 {
                fs::remove_file(&second_file).unwrap();
            }
        },
    )
    .unwrap_err();

    assert!(error.to_string().contains("cannot read"));
    assert_eq!(fs::read(&output).unwrap(), b"existing archive");
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 2);
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
fn cli_writes_checksum_manifest_beside_archive() {
    let directory = tempdir().unwrap();
    let source = directory.path().join("input");
    fs::write(&source, b"checksum me").unwrap();
    let archive = directory.path().join("result.tar.gz");
    let manifest = directory.path().join("SHA256SUMS");

    let output = Command::new(env!("CARGO_BIN_EXE_dtar"))
        .args(["--checksum", "--output"])
        .arg(&archive)
        .arg(&source)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "dtar failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let archive_bytes = fs::read(&archive).unwrap();
    let expected = format!(
        "{:x}  result.tar.gz\n",
        Sha256::digest(archive_bytes.as_slice())
    );
    assert_eq!(fs::read_to_string(&manifest).unwrap(), expected);
    assert!(String::from_utf8_lossy(&output.stdout).contains("Created checksum manifest"));
}

#[test]
fn checksum_manifest_collision_requires_force_and_dry_run_creates_nothing() {
    let directory = tempdir().unwrap();
    let source = directory.path().join("input");
    fs::write(&source, b"checksum me").unwrap();
    let archive = directory.path().join("result.tar.gz");
    let manifest = directory.path().join("SHA256SUMS");
    fs::write(&manifest, "existing manifest").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_dtar"))
        .args(["--dry-run", "--checksum", "--output"])
        .arg(&archive)
        .arg(&source)
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("use --force to replace it"));
    assert!(!archive.exists());
    assert_eq!(fs::read_to_string(&manifest).unwrap(), "existing manifest");

    let preview = Command::new(env!("CARGO_BIN_EXE_dtar"))
        .args(["--dry-run", "--checksum", "--force", "--output"])
        .arg(&archive)
        .arg(&source)
        .output()
        .unwrap();
    assert!(
        preview.status.success(),
        "dtar failed: {}",
        String::from_utf8_lossy(&preview.stderr)
    );
    assert!(String::from_utf8_lossy(&preview.stdout).contains("Checksum manifest:"));
    assert!(!archive.exists());
    assert_eq!(fs::read_to_string(manifest).unwrap(), "existing manifest");
}

#[test]
fn cli_requires_at_least_one_source() {
    let output = Command::new(env!("CARGO_BIN_EXE_dtar")).output().unwrap();

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Usage: dtar"));
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
fn cli_excludes_os_artifacts_in_dry_run_and_archive() {
    let directory = tempdir().unwrap();
    let source = directory.path().join("input");
    fs::create_dir_all(source.join("ordinary")).unwrap();
    fs::write(source.join("ordinary/keep.txt"), b"keep").unwrap();
    let artifacts = [
        ".DS_Store",
        "._resource",
        ".Spotlight-V100/index.plist",
        ".fseventsd/log",
        ".Trashes/deleted.txt",
        ".TemporaryItems/temp",
        "Thumbs.db",
        "ehthumbs.db",
        "desktop.ini",
        "$RECYCLE.BIN/deleted.txt",
        "System Volume Information/index.dat",
        ".directory",
        ".Trash-1000/deleted.txt",
        "lost+found/recovered.txt",
        "ordinary/skip.tmp",
    ];
    for artifact in artifacts {
        let path = source.join(artifact);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"excluded").unwrap();
    }
    let output_path = directory.path().join("result.tar.gz");

    let preview = Command::new(env!("CARGO_BIN_EXE_dtar"))
        .args(["--dry-run", "--exclude-os-artifacts", "--output"])
        .arg(&output_path)
        .args(["--exclude", "*.tmp"])
        .arg(&source)
        .output()
        .unwrap();

    assert!(
        preview.status.success(),
        "dtar dry run failed: {}",
        String::from_utf8_lossy(&preview.stderr)
    );
    let stdout = String::from_utf8(preview.stdout).unwrap();
    assert!(stdout.contains("ordinary/"));
    assert!(stdout.contains("keep.txt"));
    assert!(!stdout.contains("DS_Store"));
    assert!(!stdout.contains("Thumbs.db"));
    assert!(!stdout.contains("RECYCLE.BIN"));
    assert!(!stdout.contains("lost+found"));
    assert!(!stdout.contains("skip.tmp"));
    assert!(!output_path.exists());

    let archive_result = Command::new(env!("CARGO_BIN_EXE_dtar"))
        .args(["--exclude-os-artifacts", "--output"])
        .arg(&output_path)
        .args(["--exclude", "*.tmp"])
        .arg(&source)
        .output()
        .unwrap();
    assert!(
        archive_result.status.success(),
        "dtar failed: {}",
        String::from_utf8_lossy(&archive_result.stderr)
    );

    let decoder = GzDecoder::new(File::open(output_path).unwrap());
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
    assert_eq!(paths, ["ordinary", "ordinary/keep.txt"]);
}

#[test]
fn cli_archives_multiple_file_and_directory_sources() {
    let directory = tempdir().unwrap();
    let code = directory.path().join("code");
    let assets = directory.path().join("assets");
    fs::create_dir_all(&code).unwrap();
    fs::create_dir_all(&assets).unwrap();
    let main_file = code.join("main.rs");
    let readme = directory.path().join("README.md");
    fs::write(&main_file, b"main").unwrap();
    fs::write(&readme, b"readme").unwrap();
    fs::write(assets.join("logo.png"), b"logo").unwrap();
    let sources = [&main_file, &readme, &assets];
    let output_path = directory.path().join("multiple.tar.gz");

    let output = Command::new(env!("CARGO_BIN_EXE_dtar"))
        .args(["--dry-run", "--output"])
        .arg(&output_path)
        .args(sources)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "dtar failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("README.md"));
    assert!(stdout.contains("main.rs"));
    assert!(stdout.contains("logo.png"));
    assert!(stdout.contains("assets/"));
    assert!(stdout.contains("Archive/"));
    assert!(!output_path.exists());

    let output = Command::new(env!("CARGO_BIN_EXE_dtar"))
        .args(sources)
        .args(["--quiet", "--output"])
        .arg(&output_path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "dtar failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let decoder = GzDecoder::new(File::open(output_path).unwrap());
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
    assert_eq!(paths, ["README.md", "assets", "assets/logo.png", "main.rs"]);
}

#[test]
fn cli_requires_output_for_multiple_sources() {
    let directory = tempdir().unwrap();
    let first_dir = directory.path().join("first");
    let second_dir = directory.path().join("second");
    fs::create_dir_all(&first_dir).unwrap();
    fs::create_dir_all(&second_dir).unwrap();
    let first = first_dir.join("one.txt");
    let second = second_dir.join("two.txt");
    fs::write(&first, b"one").unwrap();
    fs::write(&second, b"two").unwrap();

    let missing_output = Command::new(env!("CARGO_BIN_EXE_dtar"))
        .arg("--quiet")
        .arg(&first)
        .arg(&second)
        .output()
        .unwrap();

    assert!(!missing_output.status.success());
    assert!(String::from_utf8_lossy(&missing_output.stderr)
        .contains("--output is required when multiple source paths are provided"));
    assert!(!first_dir.join("one.txt.tar.gz").exists());

    let dry_run_without_output = Command::new(env!("CARGO_BIN_EXE_dtar"))
        .args(["--dry-run"])
        .arg(&first)
        .arg(&second)
        .output()
        .unwrap();
    assert!(!dry_run_without_output.status.success());
    assert!(String::from_utf8_lossy(&dry_run_without_output.stderr)
        .contains("--output is required when multiple source paths are provided"));

    let archive_path = directory.path().join("combined.tar.gz");
    let output = Command::new(env!("CARGO_BIN_EXE_dtar"))
        .args(["--quiet", "--output"])
        .arg(&archive_path)
        .arg(&first)
        .arg(&second)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "dtar failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(archive_path.is_file());
    let decoder = GzDecoder::new(File::open(archive_path).unwrap());
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
    assert_eq!(paths, ["one.txt", "two.txt"]);
}

#[test]
fn cli_keeps_default_output_for_a_single_file() {
    let directory = tempdir().unwrap();
    let source = directory.path().join("source.txt");
    fs::write(&source, b"content").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_dtar"))
        .arg("--quiet")
        .arg(&source)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "dtar failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(directory.path().join("source.txt.tar.gz").is_file());
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

#[test]
fn exclude_patterns_use_gitignore_style_path_matching() {
    let directory = tempdir().unwrap();
    let source = directory.path().join("input");
    fs::create_dir_all(source.join("temp/deep")).unwrap();
    fs::create_dir_all(source.join("logs/nested")).unwrap();
    fs::write(source.join(".DS_Store"), b"ignored").unwrap();
    fs::write(source.join("temp/.DS_Store"), b"ignored").unwrap();
    fs::write(source.join("temp/deep/.DS_Store"), b"ignored").unwrap();
    fs::write(source.join("temp/drop.tmp"), b"ignored").unwrap();
    fs::write(source.join("temp/deep/keep.tmp"), b"kept").unwrap();
    fs::write(source.join("logs/error.log"), b"ignored").unwrap();
    fs::write(source.join("logs/nested/error.log"), b"ignored").unwrap();
    let output = directory.path().join("filtered.tar.gz");
    let excludes = vec![
        ".DS_Store".to_owned(),
        "temp/*.tmp".to_owned(),
        "logs/**/*.log".to_owned(),
    ];

    let stats = dtar::compress_directory_with_excludes(&source, &output, false, &excludes).unwrap();

    assert_eq!(stats.files, 1);
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
    assert_eq!(
        paths,
        [
            "logs",
            "logs/nested",
            "temp",
            "temp/deep",
            "temp/deep/keep.tmp"
        ]
    );
}
