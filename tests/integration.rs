use std::process::Command;

#[test]
fn compiles_and_runs_example_program() {
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args(["run", "main.ss"])
        .output()
        .expect("failed to start subsea");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "Hello World!\nPrinted directly!\njmp works!\ncount = 6\n"
    );
}

#[test]
fn rejects_unknown_symbols_before_assembly() {
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
