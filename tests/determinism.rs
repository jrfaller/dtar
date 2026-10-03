use std::{
    fs::{self, File},
    io::Read,
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

    let first_hash = dtar::compress_directory(&source, &first, false).unwrap();
    let old = SystemTime::UNIX_EPOCH + Duration::from_secs(42);
    File::options()
        .write(true)
        .open(&file)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(old))
        .unwrap();
    let second_hash = dtar::compress_directory(&source, &second, false).unwrap();

    assert_eq!(first_hash, second_hash);
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
    dtar::compress_directory(&source, &output, false).unwrap();

    let bytes = fs::read(&output).unwrap();
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
