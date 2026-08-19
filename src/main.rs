use std::{
    env,
    path::PathBuf,
    process,
    time::{Duration, Instant},
};

use subsea::backend::Target;
use subsea::codegen::emit_target_asm_with_origins;
use subsea::driver::{
    self, BuildOutputKind, FreestandingLinkOptions, FreestandingOutputFormat, build_executable,
    build_executable_for_target, build_freestanding_executable, build_object_for_target,
    run_executable,
};
use subsea::imports;

fn main() {
    match parse_cli(env::args().skip(1).collect()) {
        Ok(CommandLine::EmitAsm {
            source_path,
            target,
            entry_symbol,
        }) => match compile_to_asm(&source_path, target, entry_symbol.as_deref()) {
            Ok(asm) => print!("{asm}"),
            Err(error) => exit_with_error(error),
        },
        Ok(CommandLine::Help) => print_usage_and_exit(0),
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

            match compile_to_asm_with_timings(&source_path, target, entry_symbol.as_deref())
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
        Ok(CommandLine::Run { source_path }) => {
            match compile_to_asm(&source_path, Target::X86_64, None)
                .and_then(|asm| build_executable(&asm, None))
            {
                Ok(output) => match run_executable(&output.output_path) {
                    Ok(status) => {
                        if let Err(error) = driver::remove_build_dir(&output.build_dir) {
                            eprintln!("Warning: {error}");
                        }

                        process::exit(status.code().unwrap_or(1));
                    }
                    Err(error) => exit_with_error(error),
                },
                Err(error) => exit_with_error(error),
            }
        }
        Err(error) => {
            eprintln!("Error: {error}");
            print_usage_and_exit(1);
        }
    }
}

enum CommandLine {
    EmitAsm {
        source_path: String,
        target: Target,
        entry_symbol: Option<String>,
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
    Help,
    Run {
        source_path: String,
    },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BuildOutputFormat {
    Elf,
    Binary,
}

fn parse_cli(args: Vec<String>) -> Result<CommandLine, String> {
    match args.as_slice() {
        [flag] if flag == "--help" || flag == "-h" => Ok(CommandLine::Help),
        [command, rest @ ..] if command == "emit-asm" => parse_emit_asm_command(rest),
        [command, source_path] if command == "run" => Ok(CommandLine::Run {
            source_path: source_path.clone(),
        }),
        [command, rest @ ..] if command == "build" => parse_build_command(rest),
        [command, ..] => Err(format!("Unknown or invalid command {command:?}")),
        [] => Err(String::from("Missing command")),
    }
}

fn parse_emit_asm_command(args: &[String]) -> Result<CommandLine, String> {
    let (source_path, target, entry_symbol) = parse_source_target_and_entry(args, "emit-asm")?;

    Ok(CommandLine::EmitAsm {
        source_path,
        target,
        entry_symbol,
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
) -> Result<(String, Target, Option<String>), String> {
    let mut source_path = None;
    let mut target = Target::X86_64;
    let mut target_provided = false;
    let mut entry_symbol = None;
    let mut position = 0;

    while position < args.len() {
        match args[position].as_str() {
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
    Ok((source_path, target, entry_symbol))
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
) -> Result<String, String> {
    compile_to_asm_with_timings(source_path, target, entry_symbol)
        .map(|compilation| compilation.asm)
}

struct CompilationOutput {
    asm: String,
    timings: CompileTimings,
}

struct CompileTimings {
    read_source: Duration,
    lex: Duration,
    parse_ast: Duration,
    codegen: Duration,
}

fn compile_to_asm_with_timings(
    source_path: &str,
    target: Target,
    entry_symbol: Option<&str>,
) -> Result<CompilationOutput, String> {
    let read_started = Instant::now();
    let source_path = PathBuf::from(source_path);
    let read_source = read_started.elapsed();

    let lex_started = Instant::now();
    let lex = lex_started.elapsed();

    let parse_started = Instant::now();
    let loaded = imports::load_program_with_origins(&source_path)?;
    let program = loaded.program;
    let parse_ast = parse_started.elapsed();

    let codegen_started = Instant::now();
    let entry_symbol = entry_symbol.unwrap_or("_start");
    let asm = emit_target_asm_with_origins(&program, target, entry_symbol, &loaded.origins)
        .map_err(|diagnostic| diagnostic.render(loaded.origins.sources()))?;
    let codegen = codegen_started.elapsed();

    Ok(CompilationOutput {
        asm,
        timings: CompileTimings {
            read_source,
            lex,
            parse_ast,
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
    println!("  read source: {:?}", compile_timings.read_source);
    println!("  lex:         {:?}", compile_timings.lex);
    println!("  parse/AST:   {:?}", compile_timings.parse_ast);
    println!("  codegen:     {:?}", compile_timings.codegen);
    println!("  assemble:    {:?}", build_timings.assemble);
    if let Some(link) = build_timings.link {
        println!("  link:        {link:?}");
    }
    println!("  total:       {total:?}");
}

fn exit_with_error(error: String) -> ! {
    let error = error.strip_prefix("error: ").unwrap_or(&error);
    eprintln!("Error: {error}");
    process::exit(1);
}

fn print_usage_and_exit(code: i32) -> ! {
    eprintln!("Usage:");
    eprintln!("  subsea run <file.ss>");
    eprintln!(
        "  subsea build [--target|-t x86|x86-free|aarch] [--entry symbol] [--linker-script|-T script.ld] [--link-input object.o]... [--format elf|binary] [--linker program] [--timings] [-o output] <file.ss>"
    );
    eprintln!("  subsea emit-asm [--target|-t x86|x86-free|aarch] [--entry symbol] <file.ss>");
    process::exit(code);
}
