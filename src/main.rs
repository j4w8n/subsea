use std::{fs, process};

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
    let source_result = fs::read_to_string("./main.ss");

    if let Ok(source) = source_result {
        println!("{source}");

        let mut tokens: Vec<Token> = Vec::new();

        let mut chars = source.chars().peekable();

        while let Some(next_token) = get_next_token(&mut chars) {
            tokens.push(next_token);
        }

        println!("Tokens: {:?}\n", tokens);

        let mut parser = Parser::new(tokens);
        match parser.parse_program() {
            Ok(program) => {
                println!("AST: {program:#?}\n");

                match emit_x86_64_linux_asm(&program) {
                    Ok(asm) => {
                        println!("Assembly:\n{asm}");

                        match build_executable(&asm) {
                            Ok(output) => {
                                println!("Wrote assembly: {}", output.asm_path.display());
                                println!("Wrote object: {}", output.object_path.display());
                                println!("Wrote executable: {}", output.executable_path.display());

                                match run_executable(&output.executable_path) {
                                    Ok(status) => {
                                        println!("Program exited with: {status}");
                                        process::exit(status.code().unwrap_or(1));
                                    }
                                    Err(error) => eprintln!("Run error: {error}"),
                                }
                            }
                            Err(error) => eprintln!("Build error: {error}"),
                        }
                    }
                    Err(error) => eprintln!("Codegen error: {error}"),
                }
            }
            Err(error) => eprintln!("Parse error: {error}"),
        }
    }
}
