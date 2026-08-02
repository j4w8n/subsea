use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

pub struct BuildOutput {
    pub asm_path: PathBuf,
    pub object_path: PathBuf,
    pub executable_path: PathBuf,
}

pub fn build_executable(asm: &str, output_path: Option<&Path>) -> Result<BuildOutput, String> {
    let build_dir = Path::new("target").join("subsea");
    fs::create_dir_all(&build_dir)
        .map_err(|error| format!("Failed to create build dir: {error}"))?;

    let asm_path = build_dir.join("main.s");
    let object_path = build_dir.join("main.o");
    let executable_path = output_path
        .map(Path::to_path_buf)
        .unwrap_or_else(|| build_dir.join("main"));

    if let Some(parent) = executable_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Failed to create output dir: {error}"))?;
    }

    fs::write(&asm_path, asm).map_err(|error| format!("Failed to write assembly: {error}"))?;

    run_command(
        Command::new("as")
            .arg(&asm_path)
            .arg("-o")
            .arg(&object_path),
        "assembler",
        "as",
    )?;

    run_command(
        Command::new("ld")
            .arg(&object_path)
            .arg("-o")
            .arg(&executable_path),
        "linker",
        "ld",
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

fn run_command(command: &mut Command, label: &str, program: &str) -> Result<(), String> {
    let output = command.output().map_err(|error| {
        if error.kind() == ErrorKind::NotFound {
            format!(
                "Failed to run {label}: `{program}` was not found. Install binutils and make sure `{program}` is on PATH."
            )
        } else {
            format!("Failed to run {label}: {error}")
        }
    })?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("{label} failed: {stderr}"))
    }
}
