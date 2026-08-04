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
  let message = "Hello World!\n"
  print message

  exit 0
}
```

This prints `Hello World!` and exits the program with status code `0`. The entry label does not have to be `main`, but is subsea's best practice.

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

Use `--timings` or `-t` with `build` to show build times for various phases of the process.

```sh
subsea build main.ss -t
```

## Source Structure

A program starts with an entry directive and label target:

```ss
.entry main
```

Labels contain instruction blocks:

```ss
main: {
  let message = "Hello World!\n"
  print message

  exit 0
}
```

Top-level `mem` declarations allocate writable memory for the lifetime of the program:

```ss
.entry main

mem count:u16 = 3
mem buf:u8(128)

main: {
  exit 0
}
```

`mem count:u16 = 3` allocates one writable `u16` memory cell initialized to `3`. `mem buf:u8(128)` allocates 128 zero-initialized writable `u8` cells.

`let` and `mem` are intentionally different:

```ss
let count:u16 = 3   // compile-time constant value
mem count:u16 = 3   // writable memory named count
```

Read and write `mem` values with memory operands:

```ss
copy [count]:u16, ax
add 1, ax
copy ax, [count]:u16
```

Use `&` to pass the address of `mem` storage:

```ss
copy &buf, rsi
```

## Bindings And Printing

Subsea supports a small first abstraction for defining and printing compile-time bindings:

```ss
let message = "Hello World!\n"
print message

print "Printed directly!\n"

let count = 3
print count

let byte_count:u8 = 3
print "count = {}\n", byte_count

let name = "Subsea"
let kind = "lang"
print "Hello, {} {}\n", name, kind
```

Bindings are currently label-local. `print message` can print a binding declared with `let` in the same label block. `print "..."` prints literal text directly.

Integer bindings can also be used as immediate operands:

```ss
let count = 3
copy count, rax
```

Integer bindings can optionally include a width annotation. Width annotations use the same names as memory widths and are checked when the binding is parsed:

```ss
let byte_count:u8 = 3
let offset:i16 = -8
```

Formatted printing supports `{}` placeholders with bindings:

```ss
let name = "Subsea"
print "Hello, {}\n", name
```

Each `{}` consumes one following binding argument. The number of placeholders must match the number of arguments. Format specifiers like `{x}` or `{i64}` are not supported yet.

Supported string escapes:

```text
\n
\t
\"
\\
```

`print` lowers to the Linux x86-64 `write` syscall and clobbers syscall registers:

```text
rax rdi rsi rdx
```

## Assembly-like Syntax

Subsea allows you to directly work with memory and registers, along with supporting other common operations.

```ss
.entry main

// This program immediately exits with an exit code of `0`
main: {
  copy 60, rax   // store the "exit" syscall
  copy 0, rdi    // store the exit status code
  syscall        // execute the syscall
}
```

## Operand Order

Subsea uses source-first, destination-second instruction syntax:

```ss
copy 10, rax
add 5, rax
sub 1, rax
```

For example, you can read the first line as "copy the value 10 to register rax". The destination is the operand that changes.

## Instructions

Currently supported instructions:

```text
copy src, dst
add src, dst
sub src, dst
umul src, dst
imul src, dst
udiv operand
idiv operand
jmp label
exit code
syscall
```

`i` means signed integer and `u` means unsigned integer.

`umul` and `imul` currently emit two-operand `imul`, because the low half of integer multiplication is the same for signed and unsigned multiplication.

`udiv` lowers to x86-64 `div`. `idiv` lowers to x86-64 `idiv`. Both intentionally follow x86-64's one-operand division form. The dividend is implicitly `rdx:rax`, the quotient is written to `rax`, and the remainder is written to `rdx`.

`exit <code>` lowers to the Linux x86-64 `exit` syscall. Exit codes must be between `0` and `255`:

```ss
exit 0
exit 1
```

Negative immediate operands are supported:

```ss
copy -1, rax
add -8, rsp
sub -1, rax
```

## Width Rules

Subsea currently rejects mixed-width register operations. These are invalid:

```ss
copy rax, eax
copy eax, rax
add eax, rax
imul ax, eax
```

Memory/register operations infer width from the register:

```ss
copy [addr], rax  // 64-bit load
copy rax, [addr]  // 64-bit store
copy [addr], eax  // 32-bit load
copy eax, [addr]  // 32-bit store
```

Ambiguous or unsupported memory moves are rejected:

```ss
copy 5, [addr]      // no explicit memory width
copy [rax], [rbx]   // memory-to-memory copy is not supported
```

Use explicit memory widths when the compiler cannot infer the width:

```ss
copy 5, [addr]:u8
copy -1, [addr]:i8
copy 500, [addr]:u16
copy -500, [addr]:i16
copy 100000, [addr]:u32
copy -100000, [addr]:i32
copy 100000, [addr]:u64
copy -100000, [addr]:i64
```

Memory width annotations lower to pointer-sized assembly operands:

```ss
copy 5, [rax]:u8
copy -1, [rax]:i64
```

```asm
mov byte ptr [rax], 5
mov qword ptr [rax], -1
```

When both a register and an explicit memory width are present, their widths must match:

```ss
copy rax, [addr]:u64  // valid
copy eax, [addr]:u32  // valid
copy rax, [addr]:u32  // invalid
```

Immediate values and integer bindings must fit the destination width:

```ss
copy 255, al      // valid
copy -1, al       // valid
copy 256, al      // invalid
copy 66000, ax    // invalid

let count:u8 = 255
copy count, al    // valid
```

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
