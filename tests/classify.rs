use assert_cmd::Command;
use std::process::Command as StdCommand;
use tempfile::TempDir;

fn init_repo(dir: &std::path::Path) {
    StdCommand::new("git")
        .args(["init"])
        .current_dir(dir)
        .output()
        .unwrap();
    StdCommand::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(dir)
        .output()
        .unwrap();
    StdCommand::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(dir)
        .output()
        .unwrap();
}

fn commit(dir: &std::path::Path, msg: &str) {
    StdCommand::new("git")
        .args(["commit", "--allow-empty", "-m", msg])
        .current_dir(dir)
        .output()
        .unwrap();
}

#[test]
fn no_remote_single_commit_is_prototype() {
    let tmp = TempDir::new().unwrap();
    let repo = tmp.path().join("tiny");
    std::fs::create_dir(&repo).unwrap();
    init_repo(&repo);
    commit(&repo, "init");

    let output = Command::cargo_bin("ward")
        .unwrap()
        .args(["scan", "--json", "--no-cache"])
        .arg(tmp.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"prototype\""),
        "expected prototype verdict, got: {stdout}"
    );
}

#[test]
fn dirty_repo_is_has_local_work() {
    let tmp = TempDir::new().unwrap();
    let repo = tmp.path().join("dirty");
    std::fs::create_dir(&repo).unwrap();
    init_repo(&repo);
    commit(&repo, "init");
    std::fs::write(repo.join("file.txt"), "uncommitted").unwrap();
    StdCommand::new("git")
        .args(["add", "file.txt"])
        .current_dir(&repo)
        .output()
        .unwrap();

    let output = Command::cargo_bin("ward")
        .unwrap()
        .args(["scan", "--json", "--no-cache"])
        .arg(tmp.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"has-local-work\""),
        "expected has-local-work verdict, got: {stdout}"
    );
}

#[test]
fn no_remote_many_commits_is_no_remote() {
    let tmp = TempDir::new().unwrap();
    let repo = tmp.path().join("established");
    std::fs::create_dir(&repo).unwrap();
    init_repo(&repo);
    for i in 0..15 {
        commit(&repo, &format!("commit {i}"));
    }

    let output = Command::cargo_bin("ward")
        .unwrap()
        .args(["scan", "--json", "--no-cache"])
        .arg(tmp.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"no-remote\""),
        "expected no-remote verdict, got: {stdout}"
    );
}
