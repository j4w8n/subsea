use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::{self, Command, ExitStatus};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::backend::Target;

static BUILD_COUNTER: AtomicU64 = AtomicU64::new(0);

pub struct BuildOutput {
    pub build_dir: PathBuf,
    pub asm_path: PathBuf,
    pub object_path: PathBuf,
    pub output_path: PathBuf,
    pub output_kind: BuildOutputKind,
    pub timings: BuildTimings,
}

pub enum BuildOutputKind {
    Executable,
    Object,
    Binary,
}

pub struct BuildTimings {
    pub assemble: Duration,
    pub link: Option<Duration>,
    pub objcopy: Option<Duration>,
}

pub struct FreestandingLinkOptions<'a> {
    pub target: Target,
    pub output_path: &'a Path,
    pub linker_script: &'a Path,
    pub link_inputs: &'a [PathBuf],
    pub output_format: FreestandingOutputFormat,
    pub linker: &'a str,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FreestandingOutputFormat {
    Elf,
    Binary,
}

pub fn build_executable(asm: &str, output_path: Option<&Path>) -> Result<BuildOutput, String> {
    let toolchain = Target::X86_64.spec();
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
        Command::new(toolchain.assembler)
            .arg(&asm_path)
            .arg("-o")
            .arg(&object_path),
        "assembler",
        toolchain.assembler,
    )?;
    let assemble = assemble_started.elapsed();

    let link_started = Instant::now();
    run_command(
        Command::new(toolchain.linker)
            .arg(&object_path)
            .arg("-o")
            .arg(&executable_path),
        "linker",
        toolchain.linker,
    )?;
    let link = link_started.elapsed();

    Ok(BuildOutput {
        build_dir,
        asm_path,
        object_path,
        output_path: executable_path,
        output_kind: BuildOutputKind::Executable,
        timings: BuildTimings {
            assemble,
            link: Some(link),
            objcopy: None,
        },
    })
}

pub fn build_object(asm: &str, output_path: &Path) -> Result<BuildOutput, String> {
    let assembled = assemble_to_output_object(asm, output_path)?;

    Ok(BuildOutput {
        build_dir: assembled.build_dir,
        asm_path: assembled.asm_path,
        object_path: assembled.object_path.clone(),
        output_path: assembled.object_path,
        output_kind: BuildOutputKind::Object,
        timings: BuildTimings {
            assemble: assembled.assemble,
            link: None,
            objcopy: None,
        },
    })
}

pub fn build_freestanding_executable(
    asm: &str,
    options: FreestandingLinkOptions<'_>,
) -> Result<BuildOutput, String> {
    let toolchain = options.target.spec();
    let build_dir = Path::new("target").join("subsea").join(unique_build_id()?);
    fs::create_dir_all(&build_dir)
        .map_err(|error| format!("Failed to create build dir: {error}"))?;

    let asm_path = build_dir.join("main.s");
    let object_path = build_dir.join("main.o");
    let linked_path = match options.output_format {
        FreestandingOutputFormat::Elf => options.output_path.to_path_buf(),
        FreestandingOutputFormat::Binary => build_dir.join("main.elf"),
    };

    create_output_parent(options.output_path)?;

    fs::write(&asm_path, asm).map_err(|error| format!("Failed to write assembly: {error}"))?;

    let assemble_started = Instant::now();
    run_command(
        Command::new(toolchain.assembler)
            .arg(&asm_path)
            .arg("-o")
            .arg(&object_path),
        "assembler",
        toolchain.assembler,
    )?;
    let assemble = assemble_started.elapsed();

    let link_started = Instant::now();
    let mut link_command = Command::new(options.linker);
    link_command
        .arg("-m")
        .arg(options.target.spec().linker_emulation)
        .arg("-T")
        .arg(options.linker_script)
        .arg(&object_path);

    for input in options.link_inputs {
        link_command.arg(input);
    }

    link_command.arg("-o").arg(&linked_path);

    run_command(&mut link_command, "linker", options.linker)?;
    let link = link_started.elapsed();

    let objcopy = if options.output_format == FreestandingOutputFormat::Binary {
        let objcopy_started = Instant::now();
        run_command(
            Command::new(toolchain.objcopy)
                .arg("-O")
                .arg("binary")
                .arg(&linked_path)
                .arg(options.output_path),
            "objcopy",
            toolchain.objcopy,
        )?;

        Some(objcopy_started.elapsed())
    } else {
        None
    };

    Ok(BuildOutput {
        build_dir,
        asm_path,
        object_path,
        output_path: options.output_path.to_path_buf(),
        output_kind: match options.output_format {
            FreestandingOutputFormat::Elf => BuildOutputKind::Executable,
            FreestandingOutputFormat::Binary => BuildOutputKind::Binary,
        },
        timings: BuildTimings {
            assemble,
            link: Some(link),
            objcopy,
        },
    })
}

struct AssembledObject {
    build_dir: PathBuf,
    asm_path: PathBuf,
    object_path: PathBuf,
    assemble: Duration,
}

fn assemble_to_output_object(asm: &str, object_path: &Path) -> Result<AssembledObject, String> {
    let toolchain = Target::X86_64.spec();
    let build_dir = Path::new("target").join("subsea").join(unique_build_id()?);
    fs::create_dir_all(&build_dir)
        .map_err(|error| format!("Failed to create build dir: {error}"))?;

    let asm_path = build_dir.join("main.s");
    let object_path = object_path.to_path_buf();

    create_output_parent(&object_path)?;

    fs::write(&asm_path, asm).map_err(|error| format!("Failed to write assembly: {error}"))?;

    let assemble_started = Instant::now();
    run_command(
        Command::new(toolchain.assembler)
            .arg(&asm_path)
            .arg("-o")
            .arg(&object_path),
        "assembler",
        toolchain.assembler,
    )?;
    let assemble = assemble_started.elapsed();

    Ok(AssembledObject {
        build_dir,
        asm_path,
        object_path,
        assemble,
    })
}

fn create_output_parent(path: &Path) -> Result<(), String> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Failed to create output dir: {error}"))?;
    }

    Ok(())
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
