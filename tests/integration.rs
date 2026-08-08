use std::collections::HashSet;
use std::path::Path;
use std::process::Command;
use std::sync::Mutex;

static CLI_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn compiles_and_runs_example_program() {
    let _guard = CLI_LOCK.lock().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args(["run", "tests/fixtures/main.ss"])
        .output()
        .expect("failed to start subsea");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "Hello World!\nPrinted directly!\njmp works!\ncount = 6\n"
    );
}

#[test]
fn help_exits_successfully() {
    let _guard = CLI_LOCK.lock().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .arg("--help")
        .output()
        .expect("failed to start subsea");

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Usage:"));
}

#[test]
fn build_accepts_flags_before_source_path() {
    let _guard = CLI_LOCK.lock().unwrap();
    let before = build_dirs();
    let output_path =
        std::env::temp_dir().join(format!("subsea-test-build-{}", std::process::id()));
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args([
            "build",
            "-t",
            "-o",
            output_path.to_str().unwrap(),
            "tests/fixtures/main.ss",
        ])
        .output()
        .expect("failed to start subsea");
    let after = build_dirs();

    let _ = std::fs::remove_file(&output_path);
    remove_build_dirs(after.difference(&before));

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("Build timings:"));
}

#[test]
fn build_rejects_duplicate_timings_flag() {
    let _guard = CLI_LOCK.lock().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args(["build", "-t", "--timings", "tests/fixtures/main.ss"])
        .output()
        .expect("failed to start subsea");

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Timings flag was already provided"));
}

#[test]
fn run_removes_its_build_directory() {
    let _guard = CLI_LOCK.lock().unwrap();
    let before = build_dirs();
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args(["run", "tests/fixtures/main.ss"])
        .output()
        .expect("failed to start subsea");
    let after = build_dirs();

    assert!(output.status.success());
    assert_eq!(before, after);
}

#[test]
fn rejects_unknown_symbols_before_assembly() {
    let _guard = CLI_LOCK.lock().unwrap();
    let source = "main: { jmp missing }\n";
    let path = std::env::temp_dir().join(format!("subsea-test-{}.ss", std::process::id()));
    std::fs::write(&path, source).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args(["emit-asm", path.to_str().unwrap()])
        .output()
        .unwrap();
    let _ = std::fs::remove_file(path);

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Unknown label"));
}

fn build_dirs() -> HashSet<String> {
    let path = Path::new("target").join("subsea");
    let Ok(entries) = std::fs::read_dir(path) else {
        return HashSet::new();
    };

    entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let file_type = entry.file_type().ok()?;
            if !file_type.is_dir() {
                return None;
            }

            let name = entry.file_name().into_string().ok()?;
            name.starts_with("build-").then_some(name)
        })
        .collect()
}

fn remove_build_dirs<'a>(names: impl Iterator<Item = &'a String>) {
    for name in names {
        let path = Path::new("target").join("subsea").join(name);
        let _ = std::fs::remove_dir_all(path);
    }
}
