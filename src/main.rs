use std::{
    env, fs,
    path::PathBuf,
    process,
    time::{Duration, Instant},
};

pub mod ast;
pub mod codegen;
pub mod driver;
pub mod grammar;
pub mod lexer;
pub mod parser;

use crate::codegen::emit_x86_64_linux_asm;
use crate::driver::{build_executable, run_executable};
use crate::grammar::Token;
use crate::lexer::get_next_token;
use crate::parser::Parser;

fn main() {
    match parse_cli(env::args().skip(1).collect()) {
        Ok(CommandLine::EmitAsm { source_path }) => match compile_to_asm(&source_path) {
            Ok(asm) => print!("{asm}"),
            Err(error) => exit_with_error(error),
        },
        Ok(CommandLine::Build {
            source_path,
            output_path,
            show_timings,
        }) => {
            let started = Instant::now();

            match compile_to_asm_with_timings(&source_path).and_then(|compilation| {
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
            match compile_to_asm(&source_path).and_then(|asm| build_executable(&asm, None)) {
                Ok(output) => match run_executable(&output.executable_path) {
                    Ok(status) => process::exit(status.code().unwrap_or(1)),
                    Err(error) => exit_with_error(error),
                },
                Err(error) => exit_with_error(error),
            }
        }
        Err(error) => {
            eprintln!("Error: {error}");
            print_usage_and_exit();
        }
    }
}

enum CommandLine {
    EmitAsm {
        source_path: String,
    },
    Build {
        source_path: String,
        output_path: Option<PathBuf>,
        show_timings: bool,
    },
    Run {
        source_path: String,
    },
}

fn parse_cli(args: Vec<String>) -> Result<CommandLine, String> {
    match args.as_slice() {
        [command, source_path] if command == "emit-asm" => Ok(CommandLine::EmitAsm {
            source_path: source_path.clone(),
        }),
        [command, source_path] if command == "run" => Ok(CommandLine::Run {
            source_path: source_path.clone(),
        }),
        [command, source_path, rest @ ..] if command == "build" => {
            parse_build_command(source_path, rest)
        }
        [command, ..] => Err(format!("Unknown or invalid command {command:?}")),
        [] => Err(String::from("Missing command")),
    }
}

fn parse_build_command(source_path: &str, args: &[String]) -> Result<CommandLine, String> {
    let mut output_path = None;
    let mut show_timings = false;
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
            "--timings" | "-t" => show_timings = true,
            flag => return Err(format!("Unknown build flag {flag:?}")),
        }

        position += 1;
    }

    Ok(CommandLine::Build {
        source_path: source_path.to_string(),
        output_path,
        show_timings,
    })
}

fn compile_to_asm(source_path: &str) -> Result<String, String> {
    compile_to_asm_with_timings(source_path).map(|compilation| compilation.asm)
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

fn compile_to_asm_with_timings(source_path: &str) -> Result<CompilationOutput, String> {
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
    let parse_ast = parse_started.elapsed();

    let codegen_started = Instant::now();
    let asm = emit_x86_64_linux_asm(&program)?;
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

fn print_usage_and_exit() -> ! {
    eprintln!("Usage:");
    eprintln!("  subsea run <file.ss>");
    eprintln!("  subsea build <file.ss> [-o output] [--timings|-t]");
    eprintln!("  subsea emit-asm <file.ss>");
    process::exit(1);
}
