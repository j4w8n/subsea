use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::{self, Command, ExitStatus};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

static BUILD_COUNTER: AtomicU64 = AtomicU64::new(0);

pub struct BuildOutput {
    pub build_dir: PathBuf,
    pub asm_path: PathBuf,
    pub object_path: PathBuf,
    pub executable_path: PathBuf,
    pub timings: BuildTimings,
}

pub struct BuildTimings {
    pub assemble: Duration,
    pub link: Duration,
}

pub fn build_executable(asm: &str, output_path: Option<&Path>) -> Result<BuildOutput, String> {
    let build_dir = Path::new("target").join("subsea").join(unique_build_id()?);
    fs::create_dir_all(&build_dir)
        .map_err(|error| format!("Failed to create build dir: {error}"))?;

    let asm_path = build_dir.join("main.s");
    let object_path = build_dir.join("main.o");
    let executable_path = output_path
        .map(Path::to_path_buf)
        .unwrap_or_else(|| Path::new("target").join("subsea").join("main"));

    if let Some(parent) = executable_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Failed to create output dir: {error}"))?;
    }

    fs::write(&asm_path, asm).map_err(|error| format!("Failed to write assembly: {error}"))?;

    let assemble_started = Instant::now();
    run_command(
        Command::new("as")
            .arg(&asm_path)
            .arg("-o")
            .arg(&object_path),
        "assembler",
        "as",
    )?;
    let assemble = assemble_started.elapsed();

    let link_started = Instant::now();
    run_command(
        Command::new("ld")
            .arg(&object_path)
            .arg("-o")
            .arg(&executable_path),
        "linker",
        "ld",
    )?;
    let link = link_started.elapsed();

    Ok(BuildOutput {
        build_dir,
        asm_path,
        object_path,
        executable_path,
        timings: BuildTimings { assemble, link },
    })
}

pub fn remove_build_dir(path: &Path) -> Result<(), String> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "Failed to remove build dir {}: {error}",
            path.display()
        )),
    }
}

fn unique_build_id() -> Result<String, String> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("System clock is before Unix epoch: {error}"))?
        .as_nanos();
    let counter = BUILD_COUNTER.fetch_add(1, Ordering::Relaxed);

    Ok(format!("build-{}-{nanos}-{counter}", process::id()))
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
