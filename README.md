<div align="center">
  <img src="img/icon.svg" alt="Wei Logo" width="120" height="120">

# Wei & cobol2wei

**A lightweight programming language and COBOL transpiler targeting modern ELF binaries**

[![Rust](https://img.shields.io/badge/Language-Rust-b7410e.svg?style=flat-square&logo=rust)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-MIT-blue.svg?style=flat-square)](LICENSE)

</div>

<br>

## Overview

**Wei** is a compiled, statically-typed programming language designed for simplicity and performance, featuring a custom lexer, recursive descent parser, and a code generation backend that produces ELF binaries.

Crucially, the Wei project includes **`cobol2wei`**, a transpiler that converts legacy COBOL code into modern Wei code, allowing you to compile and run COBOL programs as native Linux/x86_64 executables.

## Key Features

- **Modern COBOL Runtime:** Use `cobol2wei` to modernize legacy COBOL applications.
- **Native Execution:** The Wei compiler (`wei`) directly generates executable ELF files.
- **Data Division Support:** Handles complex COBOL data structures including `PIC X`, `PIC 9`, implied decimals (`V99`), `OCCURS` (arrays), and `REDEFINES`.
- **Procedure Division:** Translates COBOL verbs like `DISPLAY`, `MOVE`, `COMPUTE`, `PERFORM VARYING`, `STRING`, `INSPECT`, and `EVALUATE`.
- **File I/O:** Supports `SEQUENTIAL` and `INDEXED` file access natively within the Wei runtime.

## Toolchain Pipeline

The project consists of two main tools:

1.  **`cobol2wei` (Transpiler):**
    - Parses COBOL source files.
    - Resolves data hierarchies and 88-level condition names.
    - Emits equivalent Wei source code.
2.  **`wei` (Compiler):**
    - Lexes and parses Wei code into an Abstract Syntax Tree (AST).
    - Performs static type checking.
    - Generates machine code (x86_64) and packages it into an ELF binary (`src/elf.rs`).

## Getting Started

### Prerequisites

- [Rust](https://rustup.rs/) (latest stable)

### Installation

```bash
git clone https://github.com/Hoteira/wei
cd wei
cargo build --release
```

### Compiling COBOL to Native Binary

```bash
# 1. Transpile COBOL to Wei
./target/release/cobol2wei my_program.cbl -o my_program.wei

# 2. Compile Wei to an ELF executable
./target/release/wei my_program.wei -o my_program

# 3. Run it
./my_program
```

## License

This project is licensed under the MIT License - see the LICENSE file for details.
