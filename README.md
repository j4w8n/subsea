# Subsea

Subsea is a readable, learnable alternative to assembly that keeps direct access to CPU registers, memory, and operating-system interfaces.

Version 0.1.0 is an early release. The stable host and target are x86-64 Linux and `x86`

## Install

The GitHub release contains a prebuilt static musl x86-64 Linux binary. Stable `x86` builds require GNU `as` and `ld` at runtime.

```sh
version=0.1.0
asset="subsea-${version}-x86_64-linux.tar.gz"
base="https://github.com/j4crev/subsea/releases/download/v${version}"
curl -fLO "$base/$asset"
curl -fLO "$base/$asset.sha256"
sha256sum --check "$asset.sha256"
tar -xzf "$asset"
sudo install -m 0755 subsea /usr/local/bin/subsea
subsea --version
```

`objcopy` is needed only for experimental freestanding `--format binary` output. See [Getting Started](docs/getting-started.md) for package-manager examples and setup details.

## Quickstart

Create `hello.ss`:

```ss
main: {
  linux.print "Hello from Subsea!\n"
  linux.exit 0
}
```

Build and run it:

```sh
subsea run hello.ss
```

Every program defines `main`. Because it is the process entry rather than a called function, `main` cannot `ret`; it must exit or otherwise transfer control without returning.

## Status

| Target | Release status |
| --- | --- |
| `x86` | Stable: x86-64 Linux userland |
| `x86-free` | Experimental: x86-64 freestanding |
| `aarch` | Experimental: AArch64 Linux userland |
| `aarch-free` | Experimental: AArch64 freestanding |

Known limitations include unsupported runtime floating-point printing, freestanding raw binaries that are not bootable by themselves, and a compiler CLI only. Experimental targets may change or have incomplete toolchain/runtime coverage.

## Documentation

- [Getting Started](docs/getting-started.md)
- [Language Reference](docs/language-reference.md)
- [CLI Reference](docs/cli.md)
- [Targets and Toolchains](docs/targets.md)
- [Examples](docs/examples.md), including [Subsea vs x86-64 assembly](examples/subsea-vs-x86/README.md)
- [0.1.0 release notes](docs/releases/0.1.0.md) and [changelog](CHANGELOG.md)
- [Contributing](CONTRIBUTING.md) and [security policy](SECURITY.md)

## Support

Use [GitHub Issues](https://github.com/j4crev/subsea/issues) for reproducible bugs and documentation problems. Security vulnerabilities must be reported privately as described in [SECURITY.md](SECURITY.md).
