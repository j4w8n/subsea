# Contributing To Subsea

We are not accepting public contributions at this time, but appreciate your interest in contributing to subsea. You're welcome to open a GitHub discussion or issue for feedback or other reporting.

## Development Setup

Contributors building the compiler from source need Rust 1.85 or newer and Cargo. Backend and integration work may also need the GNU assembler/linker for the tested target; experimental raw binary tests need `objcopy` when that path is chosen.

```sh
git clone https://github.com/j4crev/subsea.git
cd subsea
cargo test
```

Run the compiler from a checkout with Cargo:

```sh
cargo run -- run examples/subsea-vs-x86/01_hello.ss
cargo run -- emit-asm --annotate examples/subsea-vs-x86/02_arithmetic.ss
```

Subsea is a binary compiler project and does not expose a public Rust library.

## Changes

- Add or update tests for compiler behavior.
- Run `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test` when the required target tools are available.
- Keep stable and experimental target claims distinct. Backend implementation completion alone does not promote a target to stable.
- Preserve useful diagnostics and include source context in new errors.
- Avoid unrelated formatting or refactoring in focused fixes.

The repository's backend architecture and test expectations are documented in [`docs/internal`](docs/internal/).

## Reports

Use [GitHub Issues](https://github.com/j4crev/subsea/issues) for non-sensitive bugs. Do not disclose suspected vulnerabilities publicly; follow [SECURITY.md](SECURITY.md).

By contributing, you agree that your contribution is licensed under the repository's [MIT License](LICENSE).
