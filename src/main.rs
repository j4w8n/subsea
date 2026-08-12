use std::{
    env, fs,
    path::PathBuf,
    process,
    time::{Duration, Instant},
};

use subsea::codegen::{Target, emit_x86_64_asm, emit_x86_64_asm_with_entry_symbol};
use subsea::driver::{self, build_executable, run_executable};
use subsea::grammar::Token;
use subsea::lexer::get_next_token;
use subsea::parser::{Parser, validate_program_symbols};

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
        }) => {
            let started = Instant::now();

            match compile_to_asm_with_timings(&source_path, target, entry_symbol.as_deref())
                .and_then(|compilation| {
                    build_executable(&compilation.asm, output_path.as_deref())
                        .map(|build| (compilation.timings, build))
                }) {
                Ok((compile_timings, output)) => {
                    let total = started.elapsed();

                    if show_timings {
                        print_build_timings(&compile_timings, &output.timings, total);
                    }

                    println!(
                        "Wrote executable: {} (built in {total:?})",
                        output.executable_path.display(),
                    );
                }
                Err(error) => exit_with_error(error),
            }
        }
        Ok(CommandLine::Run { source_path }) => {
            match compile_to_asm(&source_path, Target::X86_64, None)
                .and_then(|asm| build_executable(&asm, None))
            {
                Ok(output) => match run_executable(&output.executable_path) {
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
    },
    Help,
    Run {
        source_path: String,
    },
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
    validate_entry_target(target, entry_symbol.as_deref())?;

    Ok(CommandLine::Build {
        source_path,
        output_path,
        show_timings,
        target,
        entry_symbol,
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
    if entry_symbol.is_some() && target != Target::X86_64Free {
        return Err(String::from(
            "--entry is only supported for target x86_64-free",
        ));
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
    let source = fs::read_to_string(source_path)
        .map_err(|error| format!("Failed to read {source_path:?}: {error}"))?;
    let read_source = read_started.elapsed();

    let lex_started = Instant::now();
    let mut tokens: Vec<Token> = Vec::new();
    let mut chars = source.chars().peekable();

    while let Some(next_token) = get_next_token(&mut chars)? {
        tokens.push(next_token);
    }
    let lex = lex_started.elapsed();

    let parse_started = Instant::now();
    let mut parser = Parser::new(tokens);
    let program = parser.parse_program()?;
    validate_program_symbols(&program)?;
    let parse_ast = parse_started.elapsed();

    let codegen_started = Instant::now();
    let asm = match entry_symbol {
        Some(entry_symbol) => emit_x86_64_asm_with_entry_symbol(&program, target, entry_symbol)?,
        None => emit_x86_64_asm(&program, target)?,
    };
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
    println!("  link:        {:?}", build_timings.link);
    println!("  total:       {total:?}");
}

fn exit_with_error(error: String) -> ! {
    eprintln!("Error: {error}");
    process::exit(1);
}

fn print_usage_and_exit(code: i32) -> ! {
    eprintln!("Usage:");
    eprintln!("  subsea run <file.ss>");
    eprintln!(
        "  subsea build [--target|-t x86_64|x86_64-free] [--entry symbol] [--timings] [-o output] <file.ss>"
    );
    eprintln!("  subsea emit-asm [--target|-t x86_64|x86_64-free] [--entry symbol] <file.ss>");
    process::exit(code);
}
