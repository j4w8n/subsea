use std::{env, fs, path::PathBuf, process};

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
        }) => {
            match compile_to_asm(&source_path)
                .and_then(|asm| build_executable(&asm, output_path.as_deref()))
            {
                Ok(output) => println!("Wrote executable: {}", output.executable_path.display()),
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
        [command, source_path] if command == "build" => Ok(CommandLine::Build {
            source_path: source_path.clone(),
            output_path: None,
        }),
        [command, source_path, flag, output_path] if command == "build" && flag == "-o" => {
            Ok(CommandLine::Build {
                source_path: source_path.clone(),
                output_path: Some(PathBuf::from(output_path)),
            })
        }
        [command, ..] => Err(format!("Unknown or invalid command {command:?}")),
        [] => Err(String::from("Missing command")),
    }
}

fn compile_to_asm(source_path: &str) -> Result<String, String> {
    let source = fs::read_to_string(source_path)
        .map_err(|error| format!("Failed to read {source_path:?}: {error}"))?;

    let mut tokens: Vec<Token> = Vec::new();
    let mut chars = source.chars().peekable();

    while let Some(next_token) = get_next_token(&mut chars)? {
        tokens.push(next_token);
    }

    let mut parser = Parser::new(tokens);
    let program = parser.parse_program()?;

    emit_x86_64_linux_asm(&program)
}

fn exit_with_error(error: String) -> ! {
    eprintln!("Error: {error}");
    process::exit(1);
}

fn print_usage_and_exit() -> ! {
    eprintln!("Usage:");
    eprintln!("  subsea run <file.ss>");
    eprintln!("  subsea build <file.ss> [-o output]");
    eprintln!("  subsea emit-asm <file.ss>");
    process::exit(1);
}
