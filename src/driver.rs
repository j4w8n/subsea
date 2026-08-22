use std::ffi::OsStr;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::{self, Command, ExitStatus};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::codegen::Target;

static BUILD_COUNTER: AtomicU64 = AtomicU64::new(0);

pub struct BuildOutput {
    pub build_dir: PathBuf,
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

pub fn build_executable_for_target(
    asm: &str,
    target: Target,
    output_path: Option<&Path>,
) -> Result<BuildOutput, String> {
    build_executable_for_target_impl(asm, target, output_path, false)
}

pub fn build_run_executable(asm: &str, target: Target) -> Result<BuildOutput, String> {
    build_executable_for_target_impl(asm, target, None, true)
}

fn build_executable_for_target_impl(
    asm: &str,
    target: Target,
    output_path: Option<&Path>,
    output_in_workspace: bool,
) -> Result<BuildOutput, String> {
    let toolchain = target.spec();
    let workspace = BuildWorkspace::create()?;
    let build_dir = workspace.path().to_path_buf();

    let asm_path = build_dir.join("main.s");
    let object_path = build_dir.join("main.o");
    let executable_path = if output_in_workspace {
        build_dir.join("main")
    } else {
        output_path
            .map(Path::to_path_buf)
            .unwrap_or_else(|| Path::new("target").join("subsea").join("main"))
    };

    create_output_parent(&executable_path)?;
    let mut staged_output =
        (!output_in_workspace).then(|| StagedOutput::new(&executable_path, workspace.id()));
    let linker_output = staged_output
        .as_ref()
        .map(StagedOutput::path)
        .unwrap_or(&executable_path);

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
            .arg(linker_output),
        "link",
        toolchain.linker,
    )?;
    let link = link_started.elapsed();

    if let Some(staged_output) = staged_output.as_mut() {
        staged_output.publish()?;
    }
    workspace.retain();

    Ok(BuildOutput {
        build_dir,
        output_path: executable_path,
        output_kind: BuildOutputKind::Executable,
        timings: BuildTimings {
            assemble,
            link: Some(link),
            objcopy: None,
        },
    })
}

pub fn build_object_for_target(
    asm: &str,
    target: Target,
    output_path: &Path,
) -> Result<BuildOutput, String> {
    let assembled = assemble_to_output_object(asm, target, output_path)?;

    Ok(BuildOutput {
        build_dir: assembled.build_dir,
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
    let workspace = BuildWorkspace::create()?;
    let build_dir = workspace.path().to_path_buf();

    let asm_path = build_dir.join("main.s");
    let object_path = build_dir.join("main.o");

    create_output_parent(options.output_path)?;
    let mut staged_output = StagedOutput::new(options.output_path, workspace.id());
    let linked_path = match options.output_format {
        FreestandingOutputFormat::Elf => staged_output.path().to_path_buf(),
        FreestandingOutputFormat::Binary => build_dir.join("main.elf"),
    };

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

    run_command(&mut link_command, "link", options.linker)?;
    let link = link_started.elapsed();

    let objcopy = if options.output_format == FreestandingOutputFormat::Binary {
        let objcopy_started = Instant::now();
        run_command(
            Command::new(toolchain.objcopy)
                .arg("-O")
                .arg("binary")
                .arg(&linked_path)
                .arg(staged_output.path()),
            "objcopy",
            toolchain.objcopy,
        )?;

        Some(objcopy_started.elapsed())
    } else {
        None
    };

    staged_output.publish()?;
    workspace.retain();

    Ok(BuildOutput {
        build_dir,
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
    object_path: PathBuf,
    assemble: Duration,
}

fn assemble_to_output_object(
    asm: &str,
    target: Target,
    object_path: &Path,
) -> Result<AssembledObject, String> {
    let toolchain = target.spec();
    let workspace = BuildWorkspace::create()?;
    let build_dir = workspace.path().to_path_buf();

    let asm_path = build_dir.join("main.s");
    let object_path = object_path.to_path_buf();

    create_output_parent(&object_path)?;
    let mut staged_output = StagedOutput::new(&object_path, workspace.id());

    fs::write(&asm_path, asm).map_err(|error| format!("Failed to write assembly: {error}"))?;

    let assemble_started = Instant::now();
    run_command(
        Command::new(toolchain.assembler)
            .arg(&asm_path)
            .arg("-o")
            .arg(staged_output.path()),
        "assembler",
        toolchain.assembler,
    )?;
    let assemble = assemble_started.elapsed();

    staged_output.publish()?;
    workspace.retain();

    Ok(AssembledObject {
        build_dir,
        object_path,
        assemble,
    })
}

struct BuildWorkspace {
    path: PathBuf,
    id: String,
    retained: bool,
}

impl BuildWorkspace {
    fn create() -> Result<Self, String> {
        let id = unique_build_id()?;
        let path = Path::new("target").join("subsea").join(&id);
        fs::create_dir_all(&path)
            .map_err(|error| format!("Failed to create build dir {}: {error}", path.display()))?;

        Ok(Self {
            path,
            id,
            retained: false,
        })
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn retain(mut self) {
        self.retained = true;
    }
}

impl Drop for BuildWorkspace {
    fn drop(&mut self) {
        if !self.retained {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

struct StagedOutput {
    staged_path: PathBuf,
    output_path: PathBuf,
    published: bool,
}

impl StagedOutput {
    fn new(output_path: &Path, id: &str) -> Self {
        let parent = output_path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let mut file_name = output_path
            .file_name()
            .unwrap_or_else(|| OsStr::new("output"))
            .to_os_string();
        file_name.push(format!(".subsea-{id}.tmp"));
        let staged_path = parent.join(file_name);

        Self {
            staged_path,
            output_path: output_path.to_path_buf(),
            published: false,
        }
    }

    fn path(&self) -> &Path {
        &self.staged_path
    }

    fn publish(&mut self) -> Result<(), String> {
        fs::rename(&self.staged_path, &self.output_path).map_err(|error| {
            format!(
                "Failed to publish output {}: {error}",
                self.output_path.display()
            )
        })?;
        self.published = true;
        Ok(())
    }
}

impl Drop for StagedOutput {
    fn drop(&mut self) {
        if !self.published {
            let _ = fs::remove_file(&self.staged_path);
        }
    }
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

pub fn run_executable(
    path: &Path,
    args: &[String],
    runner: Option<&str>,
) -> Result<ExitStatus, String> {
    let mut command = match runner {
        Some(runner) => {
            let mut command = Command::new(runner);
            command.arg(path);
            command
        }
        None => Command::new(path),
    };
    command.args(args).status().map_err(|error| match runner {
        Some(runner) if error.kind() == ErrorKind::NotFound => {
            format!("Failed to run executable: runner `{runner}` was not found")
        }
        Some(runner) => format!("Failed to run executable with runner `{runner}`: {error}"),
        None => format!("Failed to run executable `{}`: {error}", path.display()),
    })
}

fn run_command(command: &mut Command, label: &str, program: &str) -> Result<(), String> {
    let output = command.output().map_err(|error| {
        if error.kind() == ErrorKind::NotFound {
            format!(
                "Failed to run {label} stage with `{program}`: program was not found. Install binutils and make sure `{program}` is on PATH."
            )
        } else {
            format!("Failed to run {label} stage with `{program}`: {error}")
        }
    })?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("{label} stage `{program}` failed: {stderr}"))
    }
}
