# subsea

A more readable, learnable replacement for assembly, with the same power to directly work with CPU registers and memory. The name is a play on words: subsea is "below C".

The current compiler targets Linux x86-64 by lowering `.ss` source files to assembly, assembling with `as`, linking with `ld`, and optionally running the result.

## Requirements

Subsea currently requires Linux x86-64 and GNU binutils:

```text
as
ld
```

On many Linux systems these are available through the `binutils` package.

## Example

```ss
.entry main

main: {
  copy 60, rax
  copy 0, rdi
  syscall
}
```

This exits the program with status code `0` using the Linux x86-64 `exit` syscall:

```text
rax = 60  syscall number for exit
rdi = 0   exit status
```

## CLI

```sh
subsea run main.ss
subsea build main.ss
subsea emit-asm main.ss
```

Commands:

```text
run       Build and execute the program
build     Compile, assemble, and link an executable
emit-asm  Compile to x86-64 assembly and print it
```

`run` exits with the compiled program's exit code.

Use `-o` with `build` to choose the executable path:

```sh
subsea build main.ss -o main
```

Build artifacts are written to:

```text
target/subsea/main.s
target/subsea/main.o
target/subsea/main
```

When `-o` is used, the assembly and object files are still written under `target/subsea`, and the executable is written to the requested output path.

## Source Structure

A program starts with an entry directive and label target:

```ss
.entry main
```

Labels contain instruction blocks:

```ss
main: {
  copy 60, rax
  copy 0, rdi
  syscall
}
```

## Operand Order

Subsea uses source-first, destination-second instruction syntax:

```ss
copy 10, rax
add 5, rax
sub 1, rax
```

The destination is the operand that changes.

## Instructions

Currently supported instructions:

```text
copy src, dst
add src, dst
sub src, dst
mul src, dst
div operand
jmp label
syscall
```

Note: `div` intentionally follows x86-64's one-operand division form. The dividend is implicitly `rdx:rax`, the quotient is written to `rax`, and the remainder is written to `rdx`.

## Registers

Subsea uses real x86-64 register names.

Supported 64-bit registers:

```text
rax rbx rcx rdx rdi rsi rbp rsp
r8 r9 r10 r11 r12 r13 r14 r15
```

Supported 32-bit registers:

```text
eax ebx ecx edx edi esi ebp esp
r8d r9d r10d r11d r12d r13d r14d r15d
```

Supported 16-bit registers:

```text
ax bx cx dx di si bp sp
r8w r9w r10w r11w r12w r13w r14w r15w
```

Supported 8-bit registers:

```text
al bl cl dl ah bh ch dh dil sil bpl spl
r8b r9b r10b r11b r12b r13b r14b r15b
```

## Memory And Pointers

Subsea uses `[]` for dereference and `&` for address-of labels.

Examples:

```ss
copy [5], rax
copy [rax], rbx
copy [label], rcx
copy &label, rdx
```

Important: numbers inside brackets are memory addresses, not immediate values.

```ss
copy 5, rax    // immediate value 5
copy [5], rax  // memory at address 5
```

Invalid address-of forms:

```ss
copy &5, rax
copy &rax, rax
copy &[rax], rax
```

Registers can contain addresses, but registers themselves are not addressable memory locations.

## Memory Arithmetic

Memory operands support x86-64-style address expressions:

```ss
copy [rax + 8], rbx
copy [rbp - 16], rcx
copy [rax + rbx + 8], rdx
copy [label + 4], r8
```

Scaled index addressing is also supported:

```ss
copy [rax + rcx * 1], rbx
copy [rax + rcx * 2], rbx
copy [rax + rcx * 4 + 8], rbx
copy [label + rcx * 8 - 16], rbx
```

Allowed scales are:

```text
1 2 4 8
```

Only registers can be scaled. These are invalid:

```ss
copy [rax + rcx * 3], rbx
copy [rax + label * 4], rbx
copy [rax + 8 * 4], rbx
```

Nested dereferences and address-of inside memory operands are not supported:

```ss
copy [[rax]], rbx
copy [&label], rbx
```

## Code Comments

```ss
// inline comment

/*
  multiline
  comment
*/
```
