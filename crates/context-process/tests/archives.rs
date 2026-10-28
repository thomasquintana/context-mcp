//! Untrusted repository bytes are evidence, never executable or filesystem paths.
use std::{io::Write, process::Command};

fn run(entries: &[(&str, &[u8])]) -> (tempfile::TempDir, std::process::Output) {
    let dir = tempfile::tempdir().unwrap();
    let mut archive =
        zip::ZipWriter::new(std::fs::File::create(dir.path().join("source.input")).unwrap());
    for (name, bytes) in entries {
        archive
            .start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        archive.write_all(bytes).unwrap();
    }
    archive.finish().unwrap();
    std::fs::write(
        dir.path().join("request.json"),
        br#"{"asset":{"media_type":"application/zip"},"cursor":null}"#,
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_context-process"))
        .current_dir(dir.path())
        .output()
        .unwrap();
    (dir, output)
}
#[test]
fn traversal_is_rejected_and_source_programs_are_never_run() {
    let (_, bad) = run(&[("../escape", b"private")]);
    assert!(!bad.status.success());
    let (dir, output) = run(&[("run.sh", b"touch EXECUTED; echo evidence")]);
    assert!(output.status.success());
    assert!(!dir.path().join("EXECUTED").exists());
    let manifest: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        manifest["segments"][0]["text"]
            .as_str()
            .unwrap()
            .contains("touch EXECUTED")
    );
}
#[test]
fn excessive_archive_members_fail_before_extraction() {
    let names = (0..1001)
        .map(|i| format!("file-{i}.txt"))
        .collect::<Vec<_>>();
    let entries = names
        .iter()
        .map(|name| (name.as_str(), b"bounded".as_slice()))
        .collect::<Vec<_>>();
    let (_, output) = run(&entries);
    assert!(!output.status.success());
}
