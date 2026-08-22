# Getting Started

Subsea's stable setup is an x86-64 Linux host compiling for the `x86` target.

## Prerequisites

Install GNU `as` and `ld`, normally provided by a distribution's binutils package:

```sh
# Debian and Ubuntu
sudo apt install binutils

# Fedora
sudo dnf install binutils

# Arch Linux
sudo pacman -S binutils
```

GNU `objcopy` is not needed for normal `x86` programs. It is used only by the experimental freestanding `--format binary` flow.

## Install

Download the archive and its checksum from the v0.1.0 GitHub release, verify it before extraction, and install the binary:

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

## Hello World

Create `hello.ss`:

```ss
main: {
  const message = "Hello from Subsea!\n"
  linux.print message
  linux.exit 0
}
```

Run it directly:

```sh
subsea run hello.ss
```

Or build an executable and run that:

```sh
subsea build -o hello hello.ss
./hello
```

`main` is emitted as the process entry symbol `_start`. It cannot `ret`, because no caller exists to return to. End every reachable path with `linux.exit`, an equivalent terminating syscall, or an unconditional jump to non-returning code.

Continue with the [Language Reference](language-reference.md), [CLI Reference](cli.md), and [examples](examples.md).
