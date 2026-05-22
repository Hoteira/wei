use std::env;
use std::fs;
use std::process;

mod ast;
mod codegen;
mod elf;
mod lexer;
mod parser;
mod typeck;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: wei <input.wei> [-o output]");
        process::exit(2);
    }

    let input_path = &args[1];
    let output_path = parse_output_flag(&args).unwrap_or_else(|| "a.out".to_string());

    let source = fs::read_to_string(input_path).unwrap_or_else(|e| {
        eprintln!("wei: cannot read {}: {}", input_path, e);
        process::exit(1);
    });

    let tokens = lexer::lex(&source);
    let program = parser::parse(&tokens);

    if let Err(errors) = typeck::check(&program) {
        for e in &errors {
            eprintln!("wei: type error: {}", e);
        }
        process::exit(1);
    }

    let segment = codegen::emit(&program);

    if let Err(e) = elf::write_elf(&output_path, &segment) {
        eprintln!("wei: cannot write {}: {}", output_path, e);
        process::exit(1);
    }
}

fn parse_output_flag(args: &[String]) -> Option<String> {
    let mut i = 2;
    while i < args.len() {
        if args[i] == "-o" && i + 1 < args.len() {
            return Some(args[i + 1].clone());
        }
        i += 1;
    }
    None
}
