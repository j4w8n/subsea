use std::{
    env,
    path::PathBuf,
    process,
    time::{Duration, Instant},
};

mod analysis;
mod ast;
mod backend;
mod codegen;
mod diagnostic;
mod driver;
mod grammar;
mod imports;
mod ir;
mod lexer;
mod lower;
mod parser;
mod platform;
mod register;

#[cfg(test)]
mod internal_tests;

use crate::codegen::Target;
use crate::codegen::emit_target_asm_with_origins_options;
use crate::driver::{
    BuildOutputKind, FreestandingLinkOptions, FreestandingOutputFormat,
    build_executable_for_target, build_freestanding_executable, build_object_for_target,
    build_run_executable, run_executable,
};

const TOP_LEVEL_HELP: &str = "subsea - compile and run Subsea programs

Usage:
  subsea <COMMAND> [OPTIONS]
  subsea --help
  subsea --version

Commands:
  run       Compile and run a Linux program
  build     Compile and build an executable, object file, or binary
  emit-asm  Compile and write assembly to stdout
  help      Print top-level or command-specific help

Options:
  -h, --help     Print help
  -V, --version  Print version

Targets:
  x86         Stable x86-64 Linux target (default)
  x86-free    Experimental x86-64 freestanding target
  aarch       Experimental AArch64 Linux target
  aarch-free  Experimental AArch64 freestanding target

Run `subsea help <COMMAND>` for command-specific help.
";

const RUN_HELP: &str = "Compile and run a Subsea program

Usage:
  subsea run [OPTIONS] <file.ss> [-- <args>...]

Options:
  -t, --target <TARGET>  Linux target: x86 (stable, default) or aarch (experimental)
      --runner <PROGRAM> Run the executable through PROGRAM (for example, qemu-aarch64)
  -h, --help             Print help

Restrictions:
  run accepts only Linux targets; x86-free and aarch-free must be built instead.
  Arguments for the compiled program must follow `--`.
";

const BUILD_HELP: &str = "Compile and build a Subsea program

Usage:
  subsea build [OPTIONS] <file.ss>

Options:
  -t, --target <TARGET>       x86 (stable, default), x86-free (experimental),
                              aarch (experimental), or aarch-free (experimental)
  -o <PATH>                   Write output to PATH
      --timings               Print phase timings
      --entry <SYMBOL>        Set the entry symbol (freestanding targets only)
  -T, --linker-script <PATH>  Link with a script (freestanding targets only)
      --link-input <PATH>     Add an object to the link; repeatable (freestanding only)
      --format <FORMAT>       Output format: elf (default) or binary
      --linker <PROGRAM>      Use PROGRAM to link (freestanding targets only)
  -h, --help                  Print help

Restrictions:
  Linux targets produce an executable and reject freestanding-only options.
  A freestanding build without --linker-script produces an object file.
  --link-input requires --linker-script.
  --format binary requires a freestanding target and --linker-script; objcopy is run.
";

const EMIT_ASM_HELP: &str = "Compile a Subsea program and write assembly to stdout

Usage:
  subsea emit-asm [OPTIONS] <file.ss>

Options:
  -t, --target <TARGET>  x86 (stable, default), x86-free (experimental),
                         aarch (experimental), or aarch-free (experimental)
      --entry <SYMBOL>   Set the entry symbol (freestanding targets only)
      --annotate         Include source and generated-region comments
  -h, --help             Print help
";

fn main() {
    match parse_cli(env::args().skip(1).collect()) {
        Ok(CommandLine::EmitAsm {
            source_path,
            target,
            entry_symbol,
            annotate,
        }) => match compile_to_asm(&source_path, target, entry_symbol.as_deref(), annotate) {
            Ok(asm) => print!("{asm}"),
            Err(error) => exit_with_error(error),
        },
        Ok(CommandLine::Help(topic)) => print_help(topic),
        Ok(CommandLine::Version) => println!("subsea {}", env!("CARGO_PKG_VERSION")),
        Ok(CommandLine::Build {
            source_path,
            output_path,
            show_timings,
            target,
            entry_symbol,
            linker_script,
            link_inputs,
            output_format,
            linker,
        }) => {
            let started = Instant::now();

            match compile_to_asm_with_timings(&source_path, target, entry_symbol.as_deref(), false)
                .and_then(|compilation| {
                    build_output(
                        &compilation.asm,
                        target,
                        output_path.as_deref(),
                        linker_script.as_deref(),
                        &link_inputs,
                        output_format,
                        &linker,
                    )
                    .map(|build| (compilation.timings, build))
                }) {
                Ok((compile_timings, output)) => {
                    let total = started.elapsed();

                    if show_timings {
                        print_build_timings(&compile_timings, &output.timings, total);
                    }

                    let output_label = match output.output_kind {
                        BuildOutputKind::Executable => "executable",
                        BuildOutputKind::Object => "object file",
                        BuildOutputKind::Binary => "binary",
                    };

                    println!(
                        "Wrote {output_label}: {} (built in {total:?})",
                        output.output_path.display(),
                    );
                }
                Err(error) => exit_with_error(error),
            }
        }
        Ok(CommandLine::Run {
            source_path,
            target,
            runner,
            args,
        }) => {
            match compile_to_asm(&source_path, target, None, false)
                .and_then(|asm| build_run_executable(&asm, target))
            {
                Ok(output) => {
                    let run_result = run_executable(&output.output_path, &args, runner.as_deref());
                    let cleanup_result = driver::remove_build_dir(&output.build_dir);

                    match run_result {
                        Ok(status) => {
                            if let Err(error) = cleanup_result {
                                eprintln!("Warning: {error}");
                            }
                            process::exit(status.code().unwrap_or(1));
                        }
                        Err(error) => match cleanup_result {
                            Ok(()) => exit_with_error(error),
                            Err(cleanup_error) => {
                                exit_with_error(format!("{error}\n{cleanup_error}"))
                            }
                        },
                    }
                }
                Err(error) => exit_with_error(error),
            }
        }
        Err(error) => {
            eprintln!("Error: {error}");
            print_usage_to_stderr();
            process::exit(1);
        }
    }
}

enum CommandLine {
    EmitAsm {
        source_path: String,
        target: Target,
        entry_symbol: Option<String>,
        annotate: bool,
    },
    Build {
        source_path: String,
        output_path: Option<PathBuf>,
        show_timings: bool,
        target: Target,
        entry_symbol: Option<String>,
        linker_script: Option<PathBuf>,
        link_inputs: Vec<PathBuf>,
        output_format: BuildOutputFormat,
        linker: String,
    },
    Help(HelpTopic),
    Run {
        source_path: String,
        target: Target,
        runner: Option<String>,
        args: Vec<String>,
    },
    Version,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HelpTopic {
    TopLevel,
    Run,
    Build,
    EmitAsm,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BuildOutputFormat {
    Elf,
    Binary,
}

fn parse_cli(args: Vec<String>) -> Result<CommandLine, String> {
    match args.as_slice() {
        [flag] if is_help_flag(flag) => Ok(CommandLine::Help(HelpTopic::TopLevel)),
        [flag] if flag == "--version" || flag == "-V" => Ok(CommandLine::Version),
        [command] if command == "help" => Ok(CommandLine::Help(HelpTopic::TopLevel)),
        [command, topic] if command == "help" => help_topic(topic).map(CommandLine::Help),
        [command, flag] if is_help_flag(flag) => help_topic(command).map(CommandLine::Help),
        [command, ..] if command == "help" => {
            Err(String::from("Usage: subsea help [run|build|emit-asm]"))
        }
        [command, rest @ ..] if command == "emit-asm" => parse_emit_asm_command(rest),
        [command, rest @ ..] if command == "run" => parse_run_command(rest),
        [command, rest @ ..] if command == "build" => parse_build_command(rest),
        [command, ..] => Err(format!("Unknown or invalid command {command:?}")),
        [] => Err(String::from("Missing command")),
    }
}

fn is_help_flag(arg: &str) -> bool {
    arg == "--help" || arg == "-h"
}

fn help_topic(command: &str) -> Result<HelpTopic, String> {
    match command {
        "run" => Ok(HelpTopic::Run),
        "build" => Ok(HelpTopic::Build),
        "emit-asm" => Ok(HelpTopic::EmitAsm),
        _ => Err(format!("Unknown help topic {command:?}")),
    }
}

fn parse_run_command(args: &[String]) -> Result<CommandLine, String> {
    let mut source_path = None;
    let mut target = Target::X86_64;
    let mut target_provided = false;
    let mut runner = None;
    let mut program_args = Vec::new();
    let mut position = 0;

    while position < args.len() {
        match args[position].as_str() {
            "--" => {
                program_args.extend_from_slice(&args[position + 1..]);
                break;
            }
            "--target" | "-t" => {
                position += 1;
                let target_name = args
                    .get(position)
                    .ok_or_else(|| String::from("Expected target after --target/-t"))?;
                if target_provided {
                    return Err(String::from("Target was already provided"));
                }
                target = Target::parse(target_name)?;
                target_provided = true;
            }
            "--runner" => {
                position += 1;
                let program = args
                    .get(position)
                    .ok_or_else(|| String::from("Expected runner program after --runner"))?;
                if runner.is_some() {
                    return Err(String::from("Runner was already provided"));
                }
                validate_program_name(program, "Runner")?;
                runner = Some(program.clone());
            }
            flag if flag.starts_with('-') => return Err(format!("Unknown run flag {flag:?}")),
            path => {
                if source_path.is_some() {
                    return Err(String::from("Source path was already provided"));
                }
                source_path = Some(path.to_owned());
            }
        }
        position += 1;
    }

    let source_path = source_path.ok_or_else(|| String::from("Missing run source path"))?;
    if target.is_freestanding() {
        return Err(String::from(
            "subsea run only supports Linux targets; use build for freestanding targets",
        ));
    }

    Ok(CommandLine::Run {
        source_path,
        target,
        runner,
        args: program_args,
    })
}

fn parse_emit_asm_command(args: &[String]) -> Result<CommandLine, String> {
    let (source_path, target, entry_symbol, annotate) =
        parse_source_target_and_entry(args, "emit-asm")?;

    Ok(CommandLine::EmitAsm {
        source_path,
        target,
        entry_symbol,
        annotate,
    })
}

fn parse_build_command(args: &[String]) -> Result<CommandLine, String> {
    let mut source_path = None;
    let mut output_path = None;
    let mut show_timings = false;
    let mut timings_provided = false;
    let mut target = Target::X86_64;
    let mut target_provided = false;
    let mut entry_symbol = None;
    let mut linker_script = None;
    let mut link_inputs = Vec::new();
    let mut output_format = BuildOutputFormat::Elf;
    let mut format_provided = false;
    let mut linker = String::from("ld");
    let mut linker_provided = false;
    let mut position = 0;

    while position < args.len() {
        match args[position].as_str() {
            "-o" => {
                position += 1;

                let path = args
                    .get(position)
                    .ok_or_else(|| String::from("Expected output path after -o"))?;

                if output_path.is_some() {
                    return Err(String::from("Output path was already provided"));
                }

                output_path = Some(PathBuf::from(path));
            }
            "--timings" => {
                if timings_provided {
                    return Err(String::from("Timings flag was already provided"));
                }

                timings_provided = true;
                show_timings = true;
            }
            "--target" | "-t" => {
                position += 1;

                let target_name = args
                    .get(position)
                    .ok_or_else(|| String::from("Expected target after --target/-t"))?;

                if target_provided {
                    return Err(String::from("Target was already provided"));
                }

                target = Target::parse(target_name)?;
                target_provided = true;
            }
            "--entry" => {
                position += 1;

                let symbol = args
                    .get(position)
                    .ok_or_else(|| String::from("Expected symbol after --entry"))?;

                if entry_symbol.is_some() {
                    return Err(String::from("Entry symbol was already provided"));
                }

                validate_entry_symbol(symbol)?;
                entry_symbol = Some(symbol.clone());
            }
            "--linker-script" | "-T" => {
                position += 1;

                let path = args.get(position).ok_or_else(|| {
                    String::from("Expected linker script path after --linker-script/-T")
                })?;

                if linker_script.is_some() {
                    return Err(String::from("Linker script was already provided"));
                }

                linker_script = Some(PathBuf::from(path));
            }
            "--format" => {
                position += 1;

                let format = args
                    .get(position)
                    .ok_or_else(|| String::from("Expected format after --format"))?;

                if format_provided {
                    return Err(String::from("Output format was already provided"));
                }

                output_format = parse_build_output_format(format)?;
                format_provided = true;
            }
            "--linker" => {
                position += 1;

                let program = args
                    .get(position)
                    .ok_or_else(|| String::from("Expected linker program after --linker"))?;

                if linker_provided {
                    return Err(String::from("Linker was already provided"));
                }

                validate_program_name(program, "Linker")?;
                linker = program.clone();
                linker_provided = true;
            }
            "--link-input" => {
                position += 1;

                let path = args
                    .get(position)
                    .ok_or_else(|| String::from("Expected object path after --link-input"))?;

                link_inputs.push(PathBuf::from(path));
            }
            flag if flag.starts_with('-') => return Err(format!("Unknown build flag {flag:?}")),
            path => {
                if source_path.is_some() {
                    return Err(String::from("Source path was already provided"));
                }

                source_path = Some(path.to_string());
            }
        }

        position += 1;
    }

    let source_path = source_path.ok_or_else(|| String::from("Missing build source path"))?;
    if !linker_provided {
        linker = target.spec().linker.to_owned();
    }
    validate_entry_target(target, entry_symbol.as_deref())?;
    validate_linker_script_target(target, linker_script.as_deref())?;
    validate_format_target(target, output_format)?;
    validate_linker_target(target, linker_provided)?;
    validate_link_inputs_target(target, &link_inputs)?;
    validate_link_inputs_require_linker_script(&link_inputs, linker_script.as_deref())?;
    validate_binary_requires_linker_script(output_format, linker_script.as_deref())?;

    Ok(CommandLine::Build {
        source_path,
        output_path,
        show_timings,
        target,
        entry_symbol,
        linker_script,
        link_inputs,
        output_format,
        linker,
    })
}

fn parse_source_target_and_entry(
    args: &[String],
    command: &str,
) -> Result<(String, Target, Option<String>, bool), String> {
    let mut source_path = None;
    let mut target = Target::X86_64;
    let mut target_provided = false;
    let mut entry_symbol = None;
    let mut annotate = false;
    let mut position = 0;

    while position < args.len() {
        match args[position].as_str() {
            "--annotate" if command == "emit-asm" => {
                if annotate {
                    return Err(String::from("Annotation was already enabled"));
                }
                annotate = true;
            }
            "--target" | "-t" => {
                position += 1;

                let target_name = args
                    .get(position)
                    .ok_or_else(|| String::from("Expected target after --target/-t"))?;

                if target_provided {
                    return Err(String::from("Target was already provided"));
                }

                target = Target::parse(target_name)?;
                target_provided = true;
            }
            "--entry" => {
                position += 1;

                let symbol = args
                    .get(position)
                    .ok_or_else(|| String::from("Expected symbol after --entry"))?;

                if entry_symbol.is_some() {
                    return Err(String::from("Entry symbol was already provided"));
                }

                validate_entry_symbol(symbol)?;
                entry_symbol = Some(symbol.clone());
            }
            flag if flag.starts_with('-') => {
                return Err(format!("Unknown {command} flag {flag:?}"));
            }
            path => {
                if source_path.is_some() {
                    return Err(String::from("Source path was already provided"));
                }

                source_path = Some(path.to_string());
            }
        }

        position += 1;
    }

    let source_path = source_path.ok_or_else(|| format!("Missing {command} source path"))?;
    validate_entry_target(target, entry_symbol.as_deref())?;
    Ok((source_path, target, entry_symbol, annotate))
}

fn validate_entry_target(target: Target, entry_symbol: Option<&str>) -> Result<(), String> {
    if entry_symbol.is_some() && !target.is_freestanding() {
        return Err(String::from(
            "--entry is only supported for freestanding targets",
        ));
    }

    Ok(())
}

fn validate_linker_script_target(
    target: Target,
    linker_script: Option<&std::path::Path>,
) -> Result<(), String> {
    if linker_script.is_some() && !target.is_freestanding() {
        return Err(String::from(
            "--linker-script/-T is only supported for freestanding targets",
        ));
    }

    Ok(())
}

fn validate_format_target(target: Target, output_format: BuildOutputFormat) -> Result<(), String> {
    if output_format == BuildOutputFormat::Binary && !target.is_freestanding() {
        return Err(String::from(
            "--format binary is only supported for freestanding targets",
        ));
    }

    Ok(())
}

fn validate_linker_target(target: Target, linker_provided: bool) -> Result<(), String> {
    if linker_provided && !target.is_freestanding() {
        return Err(String::from(
            "--linker is only supported for freestanding targets",
        ));
    }

    Ok(())
}

fn validate_link_inputs_target(target: Target, link_inputs: &[PathBuf]) -> Result<(), String> {
    if !link_inputs.is_empty() && !target.is_freestanding() {
        return Err(String::from(
            "--link-input is only supported for freestanding targets",
        ));
    }

    Ok(())
}

fn validate_link_inputs_require_linker_script(
    link_inputs: &[PathBuf],
    linker_script: Option<&std::path::Path>,
) -> Result<(), String> {
    if !link_inputs.is_empty() && linker_script.is_none() {
        return Err(String::from(
            "--link-input requires --linker-script/-T for a freestanding target",
        ));
    }

    Ok(())
}

fn validate_binary_requires_linker_script(
    output_format: BuildOutputFormat,
    linker_script: Option<&std::path::Path>,
) -> Result<(), String> {
    if output_format == BuildOutputFormat::Binary && linker_script.is_none() {
        return Err(String::from(
            "--format binary requires --linker-script/-T for a freestanding target",
        ));
    }

    Ok(())
}

fn parse_build_output_format(format: &str) -> Result<BuildOutputFormat, String> {
    match format {
        "elf" => Ok(BuildOutputFormat::Elf),
        "binary" => Ok(BuildOutputFormat::Binary),
        _ => Err(format!(
            "Unknown output format {format:?}; expected elf or binary"
        )),
    }
}

fn validate_program_name(program: &str, label: &str) -> Result<(), String> {
    if program.is_empty() {
        return Err(format!("{label} cannot be empty"));
    }

    Ok(())
}

fn validate_entry_symbol(symbol: &str) -> Result<(), String> {
    if symbol.is_empty() {
        return Err(String::from("Entry symbol cannot be empty"));
    }

    if !symbol
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '.')
    {
        return Err(format!(
            "Entry symbol {symbol:?} can only contain ASCII letters, digits, '_' and '.'"
        ));
    }

    Ok(())
}

fn build_output(
    asm: &str,
    target: Target,
    output_path: Option<&std::path::Path>,
    linker_script: Option<&std::path::Path>,
    link_inputs: &[PathBuf],
    output_format: BuildOutputFormat,
    linker: &str,
) -> Result<driver::BuildOutput, String> {
    if target.is_freestanding() {
        if let Some(linker_script) = linker_script {
            let output_path = output_path
                .map(std::path::Path::to_path_buf)
                .unwrap_or_else(|| default_freestanding_link_output(output_format));

            build_freestanding_executable(
                asm,
                FreestandingLinkOptions {
                    target,
                    output_path: &output_path,
                    linker_script,
                    link_inputs,
                    output_format: output_format.into(),
                    linker,
                },
            )
        } else {
            let object_path = output_path
                .map(std::path::Path::to_path_buf)
                .unwrap_or_else(|| std::path::Path::new("target").join("subsea").join("main.o"));

            build_object_for_target(asm, target, &object_path)
        }
    } else {
        build_executable_for_target(asm, target, output_path)
    }
}

fn default_freestanding_link_output(output_format: BuildOutputFormat) -> PathBuf {
    let file_name = match output_format {
        BuildOutputFormat::Elf => "main",
        BuildOutputFormat::Binary => "main.bin",
    };

    std::path::Path::new("target")
        .join("subsea")
        .join(file_name)
}

impl From<BuildOutputFormat> for FreestandingOutputFormat {
    fn from(format: BuildOutputFormat) -> Self {
        match format {
            BuildOutputFormat::Elf => Self::Elf,
            BuildOutputFormat::Binary => Self::Binary,
        }
    }
}

fn compile_to_asm(
    source_path: &str,
    target: Target,
    entry_symbol: Option<&str>,
    annotate: bool,
) -> Result<String, String> {
    compile_to_asm_with_timings(source_path, target, entry_symbol, annotate)
        .map(|compilation| compilation.asm)
}

struct CompilationOutput {
    asm: String,
    timings: CompileTimings,
}

struct CompileTimings {
    load_parse: Duration,
    codegen: Duration,
}

fn compile_to_asm_with_timings(
    source_path: &str,
    target: Target,
    entry_symbol: Option<&str>,
    annotate: bool,
) -> Result<CompilationOutput, String> {
    let source_path = PathBuf::from(source_path);
    let load_started = Instant::now();
    let loaded = imports::load_program_with_origins(&source_path)?;
    let program = loaded.program;
    let load_parse = load_started.elapsed();

    let codegen_started = Instant::now();
    let entry_symbol = entry_symbol.unwrap_or("_start");
    let asm = emit_target_asm_with_origins_options(
        &program,
        target,
        entry_symbol,
        &loaded.origins,
        annotate,
    )
    .map_err(|diagnostic| diagnostic.render(loaded.origins.sources()))?;
    let codegen = codegen_started.elapsed();

    Ok(CompilationOutput {
        asm,
        timings: CompileTimings {
            load_parse,
            codegen,
        },
    })
}

fn print_build_timings(
    compile_timings: &CompileTimings,
    build_timings: &driver::BuildTimings,
    total: Duration,
) {
    println!("Build timings:");
    println!("  load/lex/parse/imports: {:?}", compile_timings.load_parse);
    println!("  codegen:                {:?}", compile_timings.codegen);
    println!("  assemble:               {:?}", build_timings.assemble);
    if let Some(link) = build_timings.link {
        println!("  link:                   {link:?}");
    }
    if let Some(objcopy) = build_timings.objcopy {
        println!("  objcopy:                {objcopy:?}");
    }
    println!("  total:                  {total:?}");
}

fn exit_with_error(error: String) -> ! {
    let error = error.strip_prefix("error: ").unwrap_or(&error);
    eprintln!("Error: {error}");
    process::exit(1);
}

fn print_help(topic: HelpTopic) {
    let help = match topic {
        HelpTopic::TopLevel => TOP_LEVEL_HELP,
        HelpTopic::Run => RUN_HELP,
        HelpTopic::Build => BUILD_HELP,
        HelpTopic::EmitAsm => EMIT_ASM_HELP,
    };
    print!("{help}");
}

fn print_usage_to_stderr() {
    eprintln!("Usage: subsea <run|build|emit-asm> [OPTIONS]");
    eprintln!("Try `subsea --help` for more information.");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_parses_target_runner_and_program_arguments() {
        let args = [
            "run",
            "-t",
            "aarch",
            "--runner",
            "qemu-aarch64",
            "main.ss",
            "--",
            "argument",
            "--program-flag",
        ]
        .into_iter()
        .map(String::from)
        .collect();

        let CommandLine::Run {
            source_path,
            target,
            runner,
            args,
        } = parse_cli(args).unwrap()
        else {
            panic!("expected run command");
        };

        assert_eq!(source_path, "main.ss");
        assert_eq!(target, Target::AArch64Linux);
        assert_eq!(runner.as_deref(), Some("qemu-aarch64"));
        assert_eq!(args, ["argument", "--program-flag"]);
    }

    #[test]
    fn parses_help_and_version_forms() {
        for (args, topic) in [
            (vec!["--help"], HelpTopic::TopLevel),
            (vec!["-h"], HelpTopic::TopLevel),
            (vec!["help"], HelpTopic::TopLevel),
            (vec!["run", "--help"], HelpTopic::Run),
            (vec!["build", "-h"], HelpTopic::Build),
            (vec!["help", "emit-asm"], HelpTopic::EmitAsm),
        ] {
            let command = parse_cli(args.into_iter().map(String::from).collect()).unwrap();
            assert!(matches!(command, CommandLine::Help(actual) if actual == topic));
        }

        for flag in ["--version", "-V"] {
            assert!(matches!(
                parse_cli(vec![String::from(flag)]).unwrap(),
                CommandLine::Version
            ));
        }
    }

    #[test]
    fn rejects_invalid_help_and_version_forms() {
        for args in [
            vec!["--help", "run"],
            vec!["--version", "extra"],
            vec!["help", "unknown"],
            vec!["help", "run", "extra"],
        ] {
            assert!(parse_cli(args.into_iter().map(String::from).collect()).is_err());
        }
    }
}
