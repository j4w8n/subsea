use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

pub struct BuildOutput {
    pub asm_path: PathBuf,
    pub object_path: PathBuf,
    pub executable_path: PathBuf,
}

pub fn build_executable(asm: &str) -> Result<BuildOutput, String> {
    let build_dir = Path::new("target").join("subsea");
    fs::create_dir_all(&build_dir)
        .map_err(|error| format!("Failed to create build dir: {error}"))?;

    let asm_path = build_dir.join("main.s");
    let object_path = build_dir.join("main.o");
    let executable_path = build_dir.join("main");

    fs::write(&asm_path, asm).map_err(|error| format!("Failed to write assembly: {error}"))?;

    run_command(
        Command::new("as")
            .arg(&asm_path)
            .arg("-o")
            .arg(&object_path),
        "assembler",
    )?;

    run_command(
        Command::new("ld")
            .arg(&object_path)
            .arg("-o")
            .arg(&executable_path),
        "linker",
    )?;

    Ok(BuildOutput {
        asm_path,
        object_path,
        executable_path,
    })
}

pub fn run_executable(path: &Path) -> Result<ExitStatus, String> {
    Command::new(path)
        .status()
        .map_err(|error| format!("Failed to run executable: {error}"))
}

fn run_command(command: &mut Command, label: &str) -> Result<(), String> {
    let output = command
        .output()
        .map_err(|error| format!("Failed to run {label}: {error}"))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("{label} failed: {stderr}"))
    }
}
