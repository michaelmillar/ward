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

#[test]
fn scan_detects_custom_hooks() {
    let tmp = TempDir::new().unwrap();
    let repo = tmp.path().join("hooktest");
    std::fs::create_dir(&repo).unwrap();
    init_repo(&repo);

    StdCommand::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(&repo)
        .output()
        .unwrap();

    let hook_path = repo.join(".git/hooks/pre-commit");
    std::fs::write(&hook_path, "#!/bin/sh\necho test\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook_path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    let output = Command::cargo_bin("ward")
        .unwrap()
        .args(["scan", "--json", "--no-cache"])
        .arg(tmp.path())
        .output()
        .unwrap();

    assert!(output.status.success(), "ward scan failed");
}
