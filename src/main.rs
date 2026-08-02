use std::{env, fs, process};

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
    let args: Vec<String> = env::args().collect();

    if args.len() != 3 {
        print_usage_and_exit();
    }

    let command = &args[1];
    let source_path = &args[2];

    match command.as_str() {
        "emit-asm" => match compile_to_asm(source_path) {
            Ok(asm) => print!("{asm}"),
            Err(error) => exit_with_error(error),
        },
        "build" => match compile_to_asm(source_path).and_then(|asm| build_executable(&asm)) {
            Ok(output) => println!("Wrote executable: {}", output.executable_path.display()),
            Err(error) => exit_with_error(error),
        },
        "run" => match compile_to_asm(source_path).and_then(|asm| build_executable(&asm)) {
            Ok(output) => match run_executable(&output.executable_path) {
                Ok(status) => process::exit(status.code().unwrap_or(1)),
                Err(error) => exit_with_error(error),
            },
            Err(error) => exit_with_error(error),
        },
        _ => print_usage_and_exit(),
    }
}

fn compile_to_asm(source_path: &str) -> Result<String, String> {
    let source = fs::read_to_string(source_path)
        .map_err(|error| format!("Failed to read {source_path:?}: {error}"))?;

    let mut tokens: Vec<Token> = Vec::new();
    let mut chars = source.chars().peekable();

    while let Some(next_token) = get_next_token(&mut chars) {
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
    eprintln!("Usage: subsea <run|build|emit-asm> <file.ss>");
    process::exit(1);
}
