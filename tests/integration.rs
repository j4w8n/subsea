use std::collections::HashSet;
use std::path::Path;
use std::process::Command;
use std::sync::Mutex;

static CLI_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn emit_asm_annotation_includes_source_statement() {
    let _guard = CLI_LOCK.lock().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args(["emit-asm", "--annotate", "tests/fixtures/indexed_memory.ss"])
        .output()
        .expect("failed to start subsea");

    assert!(output.status.success());
    let assembly = String::from_utf8_lossy(&output.stdout);
    assert!(assembly.contains("/tests/fixtures/indexed_memory.ss:5"));
    assert!(assembly.contains("# values[0] = 10"));
    assert!(
        assembly.find("# values[0] = 10").unwrap()
            < assembly.find("mov qword ptr [values + 0], 10").unwrap()
    );
}

#[test]
fn annotated_assembly_marks_declarations_and_generated_regions() {
    let _guard = CLI_LOCK.lock().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args(["emit-asm", "--annotate", "tests/fixtures/indexed_memory.ss"])
        .output()
        .expect("failed to start subsea");

    assert!(output.status.success());
    let assembly = String::from_utf8_lossy(&output.stdout);
    assert!(assembly.contains("# mem values:u64(3)"));
    assert!(assembly.contains("# mem bytes:u8(8)"));
    assert!(assembly.contains("# compiler-generated: static data and text setup"));
    assert!(assembly.contains("# compiler-generated: function prologue"));

    let stack_output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args(["emit-asm", "--annotate", "tests/fixtures/stack_buffer.ss"])
        .output()
        .expect("failed to start subsea");
    assert!(stack_output.status.success());
    assert!(
        String::from_utf8_lossy(&stack_output.stdout)
            .contains("# compiler-generated: stack buffer initialization")
    );
}

#[test]
fn annotated_aarch64_assembly_uses_aarch64_comments() {
    let _guard = CLI_LOCK.lock().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args([
            "emit-asm",
            "--target",
            "aarch",
            "--annotate",
            "tests/fixtures/aarch_core.ss",
        ])
        .output()
        .expect("failed to start subsea");

    assert!(output.status.success());
    let assembly = String::from_utf8_lossy(&output.stdout);
    assert!(assembly.contains("// /"));
    assert!(assembly.contains("// x0 = 2"));
    assert!(assembly.contains("// compiler-generated: function prologue"));
    assert!(!assembly.contains("# x0 = 2"));
}

#[test]
fn annotated_imports_retain_imported_source_locations() {
    let _guard = CLI_LOCK.lock().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args([
            "emit-asm",
            "--annotate",
            "tests/fixtures/imports/use_qemu_debug.ss",
        ])
        .output()
        .expect("failed to start subsea");

    assert!(output.status.success());
    let assembly = String::from_utf8_lossy(&output.stdout);
    assert!(assembly.contains("use_qemu_debug.ss:3"));
    assert!(assembly.contains("qemu_debug.ss:1"));
    assert!(assembly.contains("qemu_debug.ss:16"));
}

#[test]
fn annotated_multiline_source_preserves_all_source_lines() {
    let _guard = CLI_LOCK.lock().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args([
            "emit-asm",
            "--annotate",
            "tests/fixtures/annotated_multiline.ss",
        ])
        .output()
        .expect("failed to start subsea");

    assert!(output.status.success());
    let assembly = String::from_utf8_lossy(&output.stdout);
    assert!(assembly.contains("# rax = ("));
    assert!(assembly.contains("# 1 + 2"));
    assert!(assembly.contains("# )"));
}

#[test]
fn annotated_x86_output_is_accepted_by_as() {
    let _guard = CLI_LOCK.lock().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args(["emit-asm", "--annotate", "tests/fixtures/main.ss"])
        .output()
        .expect("failed to start subsea");
    assert!(output.status.success());

    let base = std::env::temp_dir().join(format!("subsea-annotated-{}", std::process::id()));
    let source_path = base.with_extension("s");
    let object_path = base.with_extension("o");
    std::fs::write(&source_path, &output.stdout).expect("failed to write annotated assembly");
    let assembled = Command::new("as")
        .args(["--64"])
        .arg(&source_path)
        .arg("-o")
        .arg(&object_path)
        .output()
        .expect("failed to start assembler");
    let _ = std::fs::remove_file(&source_path);
    let _ = std::fs::remove_file(&object_path);
    assert!(
        assembled.status.success(),
        "annotated assembly failed to assemble:\n{}",
        String::from_utf8_lossy(&assembled.stderr)
    );
}

#[test]
fn layouts_contracts_and_memory_alignment_run() {
    let _guard = CLI_LOCK.lock().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args(["run", "examples/layout-contract-demo.ss"])
        .output()
        .expect("failed to start subsea");

    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "30\n");
}

#[test]
fn compiles_and_runs_example_program() {
    let _guard = CLI_LOCK.lock().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args(["run", "tests/fixtures/main.ss"])
        .output()
        .expect("failed to start subsea");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "Hello World!\nPrinted directly!\nHello from the stack!\n"
    );
}

#[test]
fn builds_aarch_core_fixture_when_cross_toolchain_is_available() {
    let _guard = CLI_LOCK.lock().unwrap();
    if Command::new("aarch64-linux-gnu-as")
        .arg("--version")
        .output()
        .is_err()
        || Command::new("aarch64-linux-gnu-ld")
            .arg("--version")
            .output()
            .is_err()
    {
        return;
    }

    let output_path =
        std::env::temp_dir().join(format!("subsea-aarch-core-{}", std::process::id()));
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args(["build", "--target", "aarch", "-o"])
        .arg(&output_path)
        .arg("tests/fixtures/aarch_core.ss")
        .output()
        .expect("failed to start subsea");

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output_path.exists());
    let _ = std::fs::remove_file(output_path);
}

#[test]
fn aarch_diagnostic_rejects_x86_register_with_source_location() {
    let _guard = CLI_LOCK.lock().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args([
            "emit-asm",
            "--target",
            "aarch",
            "tests/fixtures/aarch_invalid_register.ss",
        ])
        .output()
        .expect("failed to start subsea");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(
        stderr.contains("aarch_invalid_register.ss:2:3"),
        "stderr:\n{stderr}"
    );
    assert!(stderr.contains("Register \"rax\" is not available on target aarch"));
}

#[test]
fn aarch_diagnostic_rejects_x86_inline_assembly_with_source_location() {
    let _guard = CLI_LOCK.lock().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args([
            "emit-asm",
            "--target",
            "aarch",
            "tests/fixtures/aarch_inline_x86.ss",
        ])
        .output()
        .expect("failed to start subsea");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(
        stderr.contains("aarch_inline_x86.ss:2:3"),
        "stderr:\n{stderr}"
    );
    assert!(stderr.contains("x86 inline assembly cannot be used with target aarch"));
}

#[test]
fn aarch_lowering_diagnostic_includes_source_location() {
    let _guard = CLI_LOCK.lock().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args([
            "emit-asm",
            "--target",
            "aarch",
            "tests/fixtures/aarch_unsupported.ss",
        ])
        .output()
        .expect("failed to start subsea");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(
        stderr.contains("aarch_unsupported.ss:2:3"),
        "stderr:\n{stderr}"
    );
    assert!(stderr.contains("x86 inline assembly cannot be used with target aarch"));
}

#[test]
fn aarch_backend_diagnostic_includes_source_location() {
    let _guard = CLI_LOCK.lock().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args([
            "emit-asm",
            "--target",
            "aarch",
            "tests/fixtures/aarch_unsupported_backend.ss",
        ])
        .output()
        .expect("failed to start subsea");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(
        stderr.contains("aarch_unsupported_backend.ss:3:3"),
        "stderr:\n{stderr}"
    );
    assert!(stderr.contains(
        "AArch64 backend does not support inferred runtime printing for register or binding yet"
    ));
}

#[test]
fn compiles_and_runs_runtime_string_program() {
    let _guard = CLI_LOCK.lock().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args(["run", "tests/fixtures/runtime_strings.ss"])
        .output()
        .expect("failed to start subsea");

    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "literal\nHi\n");
}

#[test]
fn compiles_and_runs_indirect_control_flow() {
    let _guard = CLI_LOCK.lock().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args(["run", "tests/fixtures/indirect_control.ss"])
        .output()
        .expect("failed to start subsea");

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "called\njumped\n");
}

#[test]
fn emits_pair_arithmetic_from_source() {
    let _guard = CLI_LOCK.lock().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args(["emit-asm", "tests/fixtures/pair_arithmetic.ss"])
        .output()
        .expect("failed to start subsea");

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let asm = String::from_utf8_lossy(&output.stdout);
    assert!(asm.contains("  add rax, rbx\n"));
    assert!(asm.contains("  adc rdx, rcx\n"));
    assert!(asm.contains("  sub rax, rbx\n"));
    assert!(asm.contains("  sbb rdx, rcx\n"));
}

#[test]
fn compiles_and_runs_runtime_formatting() {
    let _guard = CLI_LOCK.lock().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args(["run", "tests/fixtures/runtime_formatting.ss"])
        .output()
        .expect("failed to start subsea");

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("inferred signed=-7\n"));
    assert!(stdout.contains("inferred unsigned=7\n"));
    assert!(stdout.contains("inferred ptr=0x"));
    assert!(stdout.contains("signed=-42\n"));
    assert!(stdout.contains("unsigned=18446744073709551615\n"));
    assert!(stdout.contains("hex=0x2a\n"));
    assert!(stdout.contains("binary=0b101\n"));
    assert!(stdout.contains("ptr=0x2a\n"));
    assert!(stdout.contains("narrow signed=-1 -2 -3\n"));
    assert!(stdout.contains("narrow unsigned=255\n"));
}

#[test]
fn reads_stdin_into_runtime_string() {
    let _guard = CLI_LOCK.lock().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args(["run", "tests/fixtures/read_stdin.ss"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("failed to start subsea");

    {
        use std::io::Write;

        let stdin = child.stdin.as_mut().expect("child stdin missing");
        stdin.write_all(b"typed input\n").unwrap();
    }

    let output = child.wait_with_output().unwrap();

    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "typed input\n");
}

#[test]
fn reserves_and_releases_linux_memory() {
    let _guard = CLI_LOCK.lock().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args(["run", "tests/fixtures/reserve_release.ss"])
        .output()
        .expect("failed to start subsea");

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "Hi\n");
}

#[test]
fn assigns_string_bytes_to_memory() {
    let _guard = CLI_LOCK.lock().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args(["run", "tests/fixtures/string_byte_assignment.ss"])
        .output()
        .expect("failed to start subsea");

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "Hi\nBye\n");
}

#[test]
fn uses_stack_byte_buffer_as_string_backing_storage() {
    let _guard = CLI_LOCK.lock().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args(["run", "tests/fixtures/stack_buffer.ss"])
        .output()
        .expect("failed to start subsea");

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "Hello\nJello\n");
}

#[test]
fn reads_stack_string_properties() {
    let _guard = CLI_LOCK.lock().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args(["run", "tests/fixtures/string_properties.ss"])
        .output()
        .expect("failed to start subsea");

    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "props\n6\n");
}

#[test]
fn imports_exported_function() {
    let _guard = CLI_LOCK.lock().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args(["emit-asm", "tests/fixtures/imports/use_qemu_debug.ss"])
        .output()
        .expect("failed to start subsea");

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let asm = String::from_utf8_lossy(&output.stdout);
    assert!(asm.contains("  call debug_write\n"));
    assert!(asm.contains("debug_write:\n"));
    assert!(asm.contains("__import_0_debug_write_byte:\n"));
    assert!(!asm.contains("\ndebug_write_byte:\n"));
}

#[test]
fn imports_exported_static_memory_and_data() {
    let _guard = CLI_LOCK.lock().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args(["emit-asm", "tests/fixtures/imports/use_static_exports.ss"])
        .output()
        .expect("failed to start subsea");

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let asm = String::from_utf8_lossy(&output.stdout);
    assert!(asm.contains("shared:\n"));
    assert!(asm.contains("metadata:\n"));
    assert!(asm.contains("__import_0_hidden:\n"));
    assert!(asm.contains("__import_0_hidden_metadata:\n"));
}

#[test]
fn keeps_unrequested_static_exports_private() {
    let _guard = CLI_LOCK.lock().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args([
            "emit-asm",
            "tests/fixtures/imports/use_unrequested_static_export.ss",
        ])
        .output()
        .expect("failed to start subsea");

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Unknown address symbol"));
}

#[test]
fn run_rejects_freestanding_targets() {
    let _guard = CLI_LOCK.lock().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args(["run", "--target", "x86-free", "tests/fixtures/main.ss"])
        .output()
        .expect("failed to start subsea");

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("subsea run only supports Linux targets")
    );
}

#[test]
fn rejects_importing_private_function() {
    let _guard = CLI_LOCK.lock().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args([
            "emit-asm",
            "tests/fixtures/imports/import_private_helper.ss",
        ])
        .output()
        .expect("failed to start subsea");

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("is not exported"));
}

#[test]
fn deduplicates_multiple_imports_from_same_module() {
    let _guard = CLI_LOCK.lock().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args([
            "emit-asm",
            "tests/fixtures/imports/import_same_module_twice.ss",
        ])
        .output()
        .expect("failed to start subsea");

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let asm = String::from_utf8_lossy(&output.stdout);

    assert_eq!(asm.matches("__import_0_helper:\n").count(), 1);
    assert_eq!(asm.matches("first:\n").count(), 1);
    assert_eq!(asm.matches("second:\n").count(), 1);
}

#[test]
fn rewrites_imported_private_symbols_in_conditions() {
    let _guard = CLI_LOCK.lock().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args([
            "emit-asm",
            "tests/fixtures/imports/use_condition_private_symbol.ss",
        ])
        .output()
        .expect("failed to start subsea");

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let asm = String::from_utf8_lossy(&output.stdout);

    assert!(asm.contains("__import_0_flag:\n"));
    assert!(asm.contains("  cmp qword ptr [__import_0_flag], 0\n"));
    assert!(!asm.contains("  cmp qword ptr [flag], 0\n"));
}

#[test]
fn stack_string_initialization_preserves_r10() {
    let _guard = CLI_LOCK.lock().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args(["run", "tests/fixtures/preserve_r10.ss"])
        .output()
        .expect("failed to start subsea");

    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "7\n");
}

#[test]
fn compiles_and_runs_condition_features() {
    let _guard = CLI_LOCK.lock().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args(["run", "tests/fixtures/conditions.ss"])
        .output()
        .expect("failed to start subsea");

    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "1\n9\n0\n");
}

#[test]
fn compiles_and_runs_width_conversion_features() {
    let _guard = CLI_LOCK.lock().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args(["run", "tests/fixtures/width_conversion.ss"])
        .output()
        .expect("failed to start subsea");

    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "1\n");
}

#[test]
fn compiles_and_runs_indexed_memory_features() {
    let _guard = CLI_LOCK.lock().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args(["run", "tests/fixtures/indexed_memory.ss"])
        .output()
        .expect("failed to start subsea");

    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "10\n20\n30\ni!\n");
}

#[test]
fn help_exits_successfully() {
    let _guard = CLI_LOCK.lock().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .arg("--help")
        .output()
        .expect("failed to start subsea");

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Usage:"));
}

#[test]
fn build_accepts_flags_before_source_path() {
    let _guard = CLI_LOCK.lock().unwrap();
    let before = build_dirs();
    let output_path =
        std::env::temp_dir().join(format!("subsea-test-build-{}", std::process::id()));
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args([
            "build",
            "--timings",
            "-o",
            output_path.to_str().unwrap(),
            "tests/fixtures/main.ss",
        ])
        .output()
        .expect("failed to start subsea");
    let after = build_dirs();

    let _ = std::fs::remove_file(&output_path);
    remove_build_dirs(after.difference(&before));

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("Build timings:"));
}

#[test]
fn build_rejects_duplicate_timings_flag() {
    let _guard = CLI_LOCK.lock().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args(["build", "--timings", "--timings", "tests/fixtures/main.ss"])
        .output()
        .expect("failed to start subsea");

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Timings flag was already provided"));
}

#[test]
fn emit_asm_accepts_freestanding_target() {
    let _guard = CLI_LOCK.lock().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args(["emit-asm", "--target", "x86-free", "tests/fixtures/hlt.ss"])
        .output()
        .expect("failed to start subsea");

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("  hlt\n"));
}

#[test]
fn aarch64_freestanding_target_rejects_linux_helpers() {
    let _guard = CLI_LOCK.lock().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args([
            "emit-asm",
            "--target",
            "aarch-free",
            "tests/fixtures/aarch_core.ss",
        ])
        .output()
        .expect("failed to start subsea");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(stderr.contains("AArch64 backend does not support linux.exit on freestanding target"));
}

#[test]
fn aarch64_freestanding_target_emits_core_code_and_custom_entry() {
    let _guard = CLI_LOCK.lock().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args([
            "emit-asm",
            "--target",
            "aarch-free",
            "--entry",
            "kernel_entry",
            "tests/fixtures/aarch_free.ss",
        ])
        .output()
        .expect("failed to start subsea");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(stdout.contains(".global kernel_entry\n"));
    assert!(stdout.contains("kernel_entry:\n"));
    assert!(stdout.contains("  nop\n"));
}

#[test]
fn freestanding_target_accepts_custom_entry_symbol() {
    let _guard = CLI_LOCK.lock().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args([
            "emit-asm",
            "-t",
            "x86-free",
            "--entry",
            "kernel_entry",
            "tests/fixtures/hlt.ss",
        ])
        .output()
        .expect("failed to start subsea");

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains(".global kernel_entry\n"));
    assert!(String::from_utf8_lossy(&output.stdout).contains("kernel_entry:\n"));
}

#[test]
fn linux_target_rejects_custom_entry_symbol() {
    let _guard = CLI_LOCK.lock().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args([
            "emit-asm",
            "--entry",
            "kernel_entry",
            "tests/fixtures/main.ss",
        ])
        .output()
        .expect("failed to start subsea");

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--entry is only supported"));
}

#[test]
fn freestanding_target_rejects_linux_helpers() {
    let _guard = CLI_LOCK.lock().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args(["emit-asm", "-t", "x86-free", "tests/fixtures/main.ss"])
        .output()
        .expect("failed to start subsea");

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("print is only supported"));
}

#[test]
fn codegen_diagnostic_points_to_failing_instruction() {
    let _guard = CLI_LOCK.lock().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args([
            "emit-asm",
            "-t",
            "x86-free",
            "tests/fixtures/diagnostic_reserve.ss",
        ])
        .output()
        .expect("failed to start subsea");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(!stderr.contains("Error: error:"), "stderr:\n{stderr}");
    assert!(
        stderr.contains("diagnostic_reserve.ss:2:3"),
        "stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("rax = linux.reserve(4096)"),
        "stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("reserve is only supported"),
        "stderr:\n{stderr}"
    );
}

#[test]
fn semantic_diagnostic_includes_source_location() {
    let _guard = CLI_LOCK.lock().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args(["emit-asm", "tests/fixtures/diagnostic_stack_register.ss"])
        .output()
        .expect("failed to start subsea");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(
        stderr.contains("diagnostic_stack_register.ss:2:3"),
        "stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("declares stack variables"),
        "stderr:\n{stderr}"
    );
}

#[test]
fn freestanding_build_writes_object_file() {
    let _guard = CLI_LOCK.lock().unwrap();
    let before = build_dirs();
    let output_path =
        std::env::temp_dir().join(format!("subsea-test-kernel-{}.o", std::process::id()));
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args([
            "build",
            "-t",
            "x86-free",
            "-o",
            output_path.to_str().unwrap(),
            "tests/fixtures/hlt.ss",
        ])
        .output()
        .expect("failed to start subsea");
    let after = build_dirs();

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("Wrote object file:"));

    let file_output = Command::new("file")
        .arg(&output_path)
        .output()
        .expect("failed to run file");
    let _ = std::fs::remove_file(&output_path);
    remove_build_dirs(after.difference(&before));

    assert!(file_output.status.success());
    assert!(String::from_utf8_lossy(&file_output.stdout).contains("relocatable"));
}

#[test]
fn freestanding_build_links_with_linker_script() {
    let _guard = CLI_LOCK.lock().unwrap();
    let before = build_dirs();
    let output_path =
        std::env::temp_dir().join(format!("subsea-test-kernel-{}.elf", std::process::id()));
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args([
            "build",
            "-t",
            "x86-free",
            "--linker-script",
            "tests/fixtures/kernel.ld",
            "-o",
            output_path.to_str().unwrap(),
            "tests/fixtures/hlt.ss",
        ])
        .output()
        .expect("failed to start subsea");
    let after = build_dirs();

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("Wrote executable:"));

    let file_output = Command::new("file")
        .arg(&output_path)
        .output()
        .expect("failed to run file");
    let _ = std::fs::remove_file(&output_path);
    remove_build_dirs(after.difference(&before));

    assert!(file_output.status.success());
    assert!(String::from_utf8_lossy(&file_output.stdout).contains("ELF"));
    assert!(!String::from_utf8_lossy(&file_output.stdout).contains("relocatable"));
}

#[test]
fn freestanding_build_writes_raw_binary() {
    let _guard = CLI_LOCK.lock().unwrap();
    let before = build_dirs();
    let output_path =
        std::env::temp_dir().join(format!("subsea-test-kernel-{}.bin", std::process::id()));
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args([
            "build",
            "-t",
            "x86-free",
            "-T",
            "tests/fixtures/kernel.ld",
            "--format",
            "binary",
            "-o",
            output_path.to_str().unwrap(),
            "tests/fixtures/hlt.ss",
        ])
        .output()
        .expect("failed to start subsea");
    let after = build_dirs();

    let binary = std::fs::read(&output_path).unwrap_or_default();
    let _ = std::fs::remove_file(&output_path);
    remove_build_dirs(after.difference(&before));

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("Wrote binary:"));
    assert!(!binary.is_empty());
    assert!(binary.windows(2).any(|window| window == [0xf4, 0xeb]));
}

#[test]
fn freestanding_build_accepts_custom_linker() {
    let _guard = CLI_LOCK.lock().unwrap();
    let before = build_dirs();
    let output_path = std::env::temp_dir().join(format!(
        "subsea-test-kernel-linker-{}.elf",
        std::process::id()
    ));
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args([
            "build",
            "-t",
            "x86-free",
            "--linker",
            "ld",
            "-T",
            "tests/fixtures/kernel.ld",
            "-o",
            output_path.to_str().unwrap(),
            "tests/fixtures/hlt.ss",
        ])
        .output()
        .expect("failed to start subsea");
    let after = build_dirs();

    let _ = std::fs::remove_file(&output_path);
    remove_build_dirs(after.difference(&before));

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn freestanding_build_accepts_extra_link_input() {
    let _guard = CLI_LOCK.lock().unwrap();
    let before = build_dirs();
    let unique = format!("subsea-test-link-input-{}", std::process::id());
    let temp_dir = std::env::temp_dir().join(unique);
    std::fs::create_dir_all(&temp_dir).unwrap();

    let extra_asm = temp_dir.join("extra.s");
    let extra_object = temp_dir.join("extra.o");
    let output_path = temp_dir.join("kernel.elf");
    std::fs::write(
        &extra_asm,
        ".section .extra, \"a\", @progbits\n.global extra_symbol\nextra_symbol:\n  .quad 7\n",
    )
    .unwrap();

    let assemble_output = Command::new("as")
        .args([
            extra_asm.to_str().unwrap(),
            "-o",
            extra_object.to_str().unwrap(),
        ])
        .output()
        .expect("failed to run as");

    assert!(
        assemble_output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&assemble_output.stderr)
    );

    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args([
            "build",
            "-t",
            "x86-free",
            "-T",
            "tests/fixtures/kernel.ld",
            "--link-input",
            extra_object.to_str().unwrap(),
            "-o",
            output_path.to_str().unwrap(),
            "tests/fixtures/hlt.ss",
        ])
        .output()
        .expect("failed to start subsea");
    let after = build_dirs();

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let symbols_output = Command::new("readelf")
        .args(["-s", output_path.to_str().unwrap()])
        .output()
        .expect("failed to run readelf");

    let _ = std::fs::remove_dir_all(&temp_dir);
    remove_build_dirs(after.difference(&before));

    assert!(symbols_output.status.success());
    assert!(String::from_utf8_lossy(&symbols_output.stdout).contains("extra_symbol"));
}

#[test]
fn limine_example_builds_kernel_elf() {
    let _guard = CLI_LOCK.lock().unwrap();
    let before = build_dirs();
    let kernel_elf =
        std::env::temp_dir().join(format!("subsea-test-limine-{}.elf", std::process::id()));

    let build_output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args([
            "build",
            "-t",
            "x86-free",
            "-T",
            "examples/limine/kernel.ld",
            "-o",
            kernel_elf.to_str().unwrap(),
            "examples/limine/kernel.ss",
        ])
        .output()
        .expect("failed to start subsea");
    let after = build_dirs();

    assert!(
        build_output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&build_output.stderr)
    );

    let sections_output = Command::new("readelf")
        .args(["-S", kernel_elf.to_str().unwrap()])
        .output()
        .expect("failed to run readelf");

    let _ = std::fs::remove_file(&kernel_elf);
    remove_build_dirs(after.difference(&before));

    assert!(sections_output.status.success());
    assert!(String::from_utf8_lossy(&sections_output.stdout).contains(".limine_requests"));
}

#[test]
fn freestanding_build_accepts_short_linker_script_flag() {
    let _guard = CLI_LOCK.lock().unwrap();
    let before = build_dirs();
    let output_path = std::env::temp_dir().join(format!(
        "subsea-test-kernel-short-{}.elf",
        std::process::id()
    ));
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args([
            "build",
            "-t",
            "x86-free",
            "-T",
            "tests/fixtures/kernel.ld",
            "-o",
            output_path.to_str().unwrap(),
            "tests/fixtures/hlt.ss",
        ])
        .output()
        .expect("failed to start subsea");
    let after = build_dirs();

    let _ = std::fs::remove_file(&output_path);
    remove_build_dirs(after.difference(&before));

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn linux_target_rejects_linker_script() {
    let _guard = CLI_LOCK.lock().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args([
            "build",
            "--linker-script",
            "tests/fixtures/kernel.ld",
            "tests/fixtures/main.ss",
        ])
        .output()
        .expect("failed to start subsea");

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("--linker-script/-T is only supported")
    );
}

#[test]
fn binary_format_requires_linker_script() {
    let _guard = CLI_LOCK.lock().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args([
            "build",
            "-t",
            "x86-free",
            "--format",
            "binary",
            "tests/fixtures/hlt.ss",
        ])
        .output()
        .expect("failed to start subsea");

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--format binary requires"));
}

#[test]
fn linux_target_rejects_custom_linker() {
    let _guard = CLI_LOCK.lock().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args(["build", "--linker", "ld", "tests/fixtures/main.ss"])
        .output()
        .expect("failed to start subsea");

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--linker is only supported"));
}

#[test]
fn linux_target_rejects_link_input() {
    let _guard = CLI_LOCK.lock().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args(["build", "--link-input", "extra.o", "tests/fixtures/main.ss"])
        .output()
        .expect("failed to start subsea");

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--link-input is only supported"));
}

#[test]
fn link_input_requires_linker_script() {
    let _guard = CLI_LOCK.lock().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args([
            "build",
            "-t",
            "x86-free",
            "--link-input",
            "extra.o",
            "tests/fixtures/hlt.ss",
        ])
        .output()
        .expect("failed to start subsea");

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--link-input requires"));
}

#[test]
fn run_removes_its_build_directory() {
    let _guard = CLI_LOCK.lock().unwrap();
    let before = build_dirs();
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args(["run", "tests/fixtures/main.ss"])
        .output()
        .expect("failed to start subsea");
    let after = build_dirs();

    assert!(output.status.success());
    assert_eq!(before, after);
}

#[test]
fn rejects_unknown_symbols_before_assembly() {
    let _guard = CLI_LOCK.lock().unwrap();
    let source = "main: { jmp missing }\n";
    let path = std::env::temp_dir().join(format!("subsea-test-{}.ss", std::process::id()));
    std::fs::write(&path, source).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_subsea"))
        .args(["emit-asm", path.to_str().unwrap()])
        .output()
        .unwrap();
    let _ = std::fs::remove_file(path);

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Unknown label"));
}

fn build_dirs() -> HashSet<String> {
    let path = Path::new("target").join("subsea");
    let Ok(entries) = std::fs::read_dir(path) else {
        return HashSet::new();
    };

    entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let file_type = entry.file_type().ok()?;
            if !file_type.is_dir() {
                return None;
            }

            let name = entry.file_name().into_string().ok()?;
            name.starts_with("build-").then_some(name)
        })
        .collect()
}

fn remove_build_dirs<'a>(names: impl Iterator<Item = &'a String>) {
    for name in names {
        let path = Path::new("target").join("subsea").join(name);
        let _ = std::fs::remove_dir_all(path);
    }
}
