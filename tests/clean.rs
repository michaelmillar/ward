use assert_cmd::Command;
use std::fs;
use tempfile::TempDir;

fn run_clean(root: &std::path::Path) -> String {
    let output = Command::cargo_bin("ward")
        .unwrap()
        .args(["clean"])
        .arg(root)
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout).to_string()
}

#[test]
fn detects_elixir_build_and_deps() {
    let tmp = TempDir::new().unwrap();
    let proj = tmp.path().join("phoenix_app");
    fs::create_dir_all(proj.join("_build/dev/lib")).unwrap();
    fs::create_dir_all(proj.join("deps/phoenix")).unwrap();
    fs::write(proj.join("mix.exs"), "defmodule App.MixProject do\nend\n").unwrap();
    fs::write(proj.join("_build/dev/lib/app.beam"), [0u8; 64]).unwrap();
    fs::write(proj.join("deps/phoenix/mix.exs"), "").unwrap();

    let stdout = run_clean(tmp.path());
    assert!(stdout.contains("_build"), "missing _build in: {stdout}");
    assert!(stdout.contains("deps"), "missing deps in: {stdout}");
    assert!(stdout.contains("elixir"), "missing elixir tag in: {stdout}");
}

#[test]
fn detects_dotnet_bin_and_obj() {
    let tmp = TempDir::new().unwrap();
    let proj = tmp.path().join("WebApp");
    fs::create_dir_all(proj.join("bin/Debug")).unwrap();
    fs::create_dir_all(proj.join("obj/Debug")).unwrap();
    fs::write(proj.join("WebApp.csproj"), "<Project></Project>").unwrap();
    fs::write(proj.join("bin/Debug/app.dll"), [0u8; 64]).unwrap();
    fs::write(proj.join("obj/Debug/app.pdb"), [0u8; 32]).unwrap();

    let stdout = run_clean(tmp.path());
    assert!(stdout.contains("bin"), "missing bin in: {stdout}");
    assert!(stdout.contains("obj"), "missing obj in: {stdout}");
    assert!(stdout.contains("dotnet"), "missing dotnet tag in: {stdout}");
}

#[test]
fn bin_without_csproj_is_not_detected() {
    let tmp = TempDir::new().unwrap();
    let proj = tmp.path().join("scripts");
    fs::create_dir_all(proj.join("bin")).unwrap();
    fs::write(proj.join("bin/run.sh"), "#!/bin/sh\n").unwrap();

    let stdout = run_clean(tmp.path());
    assert!(
        stdout.contains("No reclaimable artefacts found")
            || !stdout.contains(proj.join("bin").to_string_lossy().as_ref()),
        "bin should not be flagged without .csproj sibling: {stdout}"
    );
}

#[test]
fn detects_venv_by_marker_with_unusual_name() {
    let tmp = TempDir::new().unwrap();
    let proj = tmp.path().join("myproj");
    let venv = proj.join("myproj");
    fs::create_dir_all(venv.join("bin")).unwrap();
    fs::create_dir_all(venv.join("lib")).unwrap();
    fs::write(
        venv.join("pyvenv.cfg"),
        "home = /usr/bin\nversion = 3.12\n",
    )
    .unwrap();
    fs::write(venv.join("lib/payload"), [0u8; 128]).unwrap();

    let stdout = run_clean(tmp.path());
    assert!(
        stdout.contains("python"),
        "venv with non-standard name not detected: {stdout}"
    );
}

#[test]
fn detects_erl_crash_dump_file() {
    let tmp = TempDir::new().unwrap();
    let proj = tmp.path().join("server");
    fs::create_dir_all(&proj).unwrap();
    fs::write(proj.join("erl_crash.dump"), [0u8; 256]).unwrap();

    let stdout = run_clean(tmp.path());
    assert!(
        stdout.contains("erl_crash.dump"),
        "crash dump not detected: {stdout}"
    );
}
