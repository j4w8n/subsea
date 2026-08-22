use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy)]
enum ExampleKind {
    Native { stdout: &'static str, status: i32 },
    ComparisonAssembly { stdout: &'static str, status: i32 },
    Freestanding,
    ImportedLibrary,
    Documentation,
    Configuration,
    ManualAsset,
}

struct Example {
    path: &'static str,
    kind: ExampleKind,
}

const EXAMPLES: &[Example] = &[
    Example {
        path: "control-flow-array-iteration.ss",
        kind: ExampleKind::Native {
            stdout: "10\n20\n30\n40\n",
            status: 0,
        },
    },
    Example {
        path: "control-flow-break-continue.ss",
        kind: ExampleKind::Native {
            stdout: "1\n2\n3\n4\n6\n7\n8\n9\n10\n",
            status: 0,
        },
    },
    Example {
        path: "control-flow-do-while.ss",
        kind: ExampleKind::Native {
            stdout: "0\n1\n2\n3\n4\n",
            status: 0,
        },
    },
    Example {
        path: "control-flow-for.ss",
        kind: ExampleKind::Native {
            stdout: "0\n1\n2\n3\n4\n5\n6\n7\n8\n9\n",
            status: 0,
        },
    },
    Example {
        path: "control-flow-guard-clause.ss",
        kind: ExampleKind::Native {
            stdout: "fail\n",
            status: 1,
        },
    },
    Example {
        path: "control-flow-if-else.ss",
        kind: ExampleKind::Native {
            stdout: "non-zero\n",
            status: 0,
        },
    },
    Example {
        path: "control-flow-state-machine.ss",
        kind: ExampleKind::Native {
            stdout: "start\ndone\n",
            status: 0,
        },
    },
    Example {
        path: "control-flow-while.ss",
        kind: ExampleKind::Native {
            stdout: "0\n1\n2\n3\n4\n",
            status: 0,
        },
    },
    Example {
        path: "freestanding/README.md",
        kind: ExampleKind::Documentation,
    },
    Example {
        path: "freestanding/kernel.ld",
        kind: ExampleKind::Freestanding,
    },
    Example {
        path: "freestanding/kernel.ss",
        kind: ExampleKind::Freestanding,
    },
    Example {
        path: "layout-contract.ss",
        kind: ExampleKind::Native {
            stdout: "30\n",
            status: 0,
        },
    },
    Example {
        path: "layout.ss",
        kind: ExampleKind::Native {
            stdout: "Header length is valid\n",
            status: 0,
        },
    },
    Example {
        path: "lib/qemu_debug.ss",
        kind: ExampleKind::ImportedLibrary,
    },
    Example {
        path: "limine/README.md",
        kind: ExampleKind::Documentation,
    },
    Example {
        path: "limine/iso_root/EFI/BOOT/.gitkeep",
        kind: ExampleKind::ManualAsset,
    },
    Example {
        path: "limine/iso_root/boot/.gitkeep",
        kind: ExampleKind::ManualAsset,
    },
    Example {
        path: "limine/iso_root/boot/limine.conf",
        kind: ExampleKind::Configuration,
    },
    Example {
        path: "limine/kernel.ld",
        kind: ExampleKind::Freestanding,
    },
    Example {
        path: "limine/kernel.ss",
        kind: ExampleKind::Freestanding,
    },
    Example {
        path: "limine/limine.conf",
        kind: ExampleKind::Configuration,
    },
    Example {
        path: "subsea-vs-x86/01_hello.asm",
        kind: ExampleKind::ComparisonAssembly {
            stdout: "Hello from x86 assembly!\n",
            status: 0,
        },
    },
    Example {
        path: "subsea-vs-x86/01_hello.ss",
        kind: ExampleKind::Native {
            stdout: "Hello from Subsea!\n",
            status: 0,
        },
    },
    Example {
        path: "subsea-vs-x86/02_arithmetic.asm",
        kind: ExampleKind::ComparisonAssembly {
            stdout: "",
            status: 0,
        },
    },
    Example {
        path: "subsea-vs-x86/02_arithmetic.ss",
        kind: ExampleKind::Native {
            stdout: "value=44, greater_than_40=1, chosen=99\n",
            status: 0,
        },
    },
    Example {
        path: "subsea-vs-x86/03_array_sum.asm",
        kind: ExampleKind::ComparisonAssembly {
            stdout: "sum=75\n",
            status: 0,
        },
    },
    Example {
        path: "subsea-vs-x86/03_array_sum.ss",
        kind: ExampleKind::Native {
            stdout: "sum=75\n",
            status: 0,
        },
    },
    Example {
        path: "subsea-vs-x86/04_function_compare.asm",
        kind: ExampleKind::ComparisonAssembly {
            stdout: "Result is 59\n",
            status: 0,
        },
    },
    Example {
        path: "subsea-vs-x86/04_function_compare.ss",
        kind: ExampleKind::Native {
            stdout: "Result is 59\n",
            status: 0,
        },
    },
    Example {
        path: "subsea-vs-x86/05_dispatch_table.asm",
        kind: ExampleKind::ComparisonAssembly {
            stdout: "result=14\n",
            status: 0,
        },
    },
    Example {
        path: "subsea-vs-x86/05_dispatch_table.ss",
        kind: ExampleKind::Native {
            stdout: "result=14\n",
            status: 0,
        },
    },
    Example {
        path: "subsea-vs-x86/06_layout.asm",
        kind: ExampleKind::ComparisonAssembly {
            stdout: "",
            status: 0,
        },
    },
    Example {
        path: "subsea-vs-x86/06_layout.ss",
        kind: ExampleKind::Native {
            stdout: "",
            status: 0,
        },
    },
    Example {
        path: "subsea-vs-x86/README.md",
        kind: ExampleKind::Documentation,
    },
];

#[test]
fn every_example_file_is_classified() {
    let root = examples_dir();
    let mut actual = Vec::new();
    collect_files(&root, &root, &mut actual);
    actual.sort();

    let mut classified: Vec<_> = EXAMPLES
        .iter()
        .map(|example| example.path.to_owned())
        .collect();
    classified.sort();
    assert_eq!(actual, classified, "update EXAMPLES to classify every file");

    let root_config = std::fs::read(root.join("limine/limine.conf")).unwrap();
    let staged_config = std::fs::read(root.join("limine/iso_root/boot/limine.conf")).unwrap();
    assert_eq!(root_config, staged_config, "Limine configurations diverged");
}

#[test]
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn native_x86_examples_have_exact_behavior() {
    let temp = TestDir::new("native-examples");
    for example in EXAMPLES {
        let ExampleKind::Native { stdout, status } = example.kind else {
            continue;
        };
        let output = Command::new(subsea())
            .current_dir(temp.path())
            .arg("run")
            .arg(examples_dir().join(example.path))
            .output()
            .unwrap();
        assert_output(example.path, &output, status, stdout.as_bytes());
    }
}

#[test]
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn comparison_assembly_builds_and_has_exact_behavior() {
    let temp = TestDir::new("comparison-assembly");
    for example in EXAMPLES {
        let ExampleKind::ComparisonAssembly { stdout, status } = example.kind else {
            continue;
        };
        let stem = Path::new(example.path).file_stem().unwrap();
        let object = temp.path().join(stem).with_extension("o");
        let executable = temp.path().join(stem);
        let assembled = Command::new("as")
            .arg(OsStr::new("--64"))
            .arg(examples_dir().join(example.path))
            .arg(OsStr::new("-o"))
            .arg(&object)
            .output()
            .unwrap();
        assert_success(example.path, &assembled);
        let linked = Command::new("ld")
            .args([
                OsStr::new("-m"),
                OsStr::new("elf_x86_64"),
                object.as_os_str(),
                OsStr::new("-o"),
                executable.as_os_str(),
            ])
            .output()
            .unwrap();
        assert_success(example.path, &linked);
        let output = Command::new(&executable).output().unwrap();
        assert_output(example.path, &output, status, stdout.as_bytes());
    }
}

#[test]
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn freestanding_examples_build_and_are_inspectable() {
    let temp = TestDir::new("freestanding-examples");
    let freestanding_object = temp.path().join("freestanding.o");
    build_freestanding(
        temp.path(),
        "freestanding/kernel.ss",
        None,
        &freestanding_object,
        false,
    );
    assert_readelf_contains(&freestanding_object, &["REL (Relocatable file)", "_start"]);

    let freestanding_elf = temp.path().join("freestanding.elf");
    build_freestanding(
        temp.path(),
        "freestanding/kernel.ss",
        Some("freestanding/kernel.ld"),
        &freestanding_elf,
        false,
    );
    assert_readelf_contains(&freestanding_elf, &["EXEC (Executable file)", "_start"]);

    let freestanding_binary = temp.path().join("freestanding.bin");
    build_freestanding(
        temp.path(),
        "freestanding/kernel.ss",
        Some("freestanding/kernel.ld"),
        &freestanding_binary,
        true,
    );
    assert!(!std::fs::read(freestanding_binary).unwrap().is_empty());

    let limine_elf = temp.path().join("limine.elf");
    build_freestanding(
        temp.path(),
        "limine/kernel.ss",
        Some("limine/kernel.ld"),
        &limine_elf,
        false,
    );
    assert_readelf_contains(
        &limine_elf,
        &["EXEC (Executable file)", "_start", ".limine_requests"],
    );
}

#[test]
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn imported_library_compiles_through_a_consumer() {
    let temp = TestDir::new("example-library");
    let library = examples_dir().join("lib/qemu_debug.ss");
    let source = temp.path().join("consumer.ss");
    std::fs::write(
        &source,
        format!(
            "import debug_write from {:?}\n\nmain: {{\n  rsi = 0\n  rdx = 0\n  call debug_write\n.hang:\n  asm.x86 \"hlt\"\n  jmp .hang\n}}\n",
            library.to_string_lossy()
        ),
    )
    .unwrap();
    let object = temp.path().join("consumer.o");
    let output = Command::new(subsea())
        .current_dir(temp.path())
        .args(["build", "-t", "x86-free", "-o"])
        .arg(&object)
        .arg(&source)
        .output()
        .unwrap();
    assert_success("lib/qemu_debug.ss consumer", &output);
    assert_readelf_contains(&object, &["REL (Relocatable file)", "debug_write"]);
}

fn build_freestanding(
    working_dir: &Path,
    source: &str,
    linker_script: Option<&str>,
    output_path: &Path,
    binary: bool,
) {
    let mut command = Command::new(subsea());
    command
        .current_dir(working_dir)
        .args(["build", "-t", "x86-free"]);
    if let Some(linker_script) = linker_script {
        command.arg("-T").arg(examples_dir().join(linker_script));
    }
    if binary {
        command.args(["--format", "binary"]);
    }
    let output = command
        .arg("-o")
        .arg(output_path)
        .arg(examples_dir().join(source))
        .output()
        .unwrap();
    assert_success(source, &output);
}

fn assert_readelf_contains(path: &Path, expected: &[&str]) {
    let output = Command::new("readelf")
        .args(["-h", "-S", "-s"])
        .arg(path)
        .output()
        .unwrap();
    assert_success(&path.display().to_string(), &output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    for text in expected {
        assert!(
            stdout.contains(text),
            "missing {text:?} in readelf output for {}:\n{stdout}",
            path.display()
        );
    }
}

fn assert_success(label: &str, output: &Output) {
    assert!(
        output.status.success(),
        "{label} failed with {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_output(label: &str, output: &Output, status: i32, stdout: &[u8]) {
    assert_eq!(
        output.status.code(),
        Some(status),
        "{label} stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, stdout, "unexpected stdout from {label}");
}

fn collect_files(root: &Path, directory: &Path, files: &mut Vec<String>) {
    for entry in std::fs::read_dir(directory).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if entry.file_type().unwrap().is_dir() {
            collect_files(root, &path, files);
        } else {
            let relative = path.strip_prefix(root).unwrap().to_str().unwrap();
            files.push(relative.to_owned());
        }
    }
}

fn examples_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("examples")
}

fn subsea() -> &'static str {
    env!("CARGO_BIN_EXE_subsea")
}

struct TestDir(PathBuf);

impl TestDir {
    fn new(label: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "subsea-{label}-{}-{nanos}-{counter}",
            std::process::id()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
