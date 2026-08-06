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
ax = [count]:u16
ax = ax + 1
[count]:u16 = ax
```

Use `&` to pass the address of `mem` storage:

```ss
mem buf:u8(128)
rsi = &buf
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
rax = count
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

## Assembly-like Power

Subsea allows you to directly work with memory and registers, along with supporting other common operations.

```ss
.entry main

// This program immediately exits with an exit code of `0`
main: {
  rax = 60  // store the "exit" syscall
  rdi = 0   // store the exit status code
  syscall   // execute the syscall
}
```

## Assignment Syntax

Subsea uses readable assignment syntax for moving values and doing simple math:

```ss
rax = 10
rax = rax + 5
rax = rax - 1
rbx = count * 2
rdx:rax = umul rbx, rcx
rdx:rax = imul rbx, rcx
```

The left side is the destination that changes. Math assignment currently supports `+`, `-`, and low-result `*`. Use `rdx:rax = umul lhs, rhs` or `rdx:rax = imul lhs, rhs` when you need the full widened multiply result. Division remains explicit with `udiv` and `idiv` because those instructions use x86-64's implicit `rdx:rax`, `rax`, and `rdx` registers.

## Instructions

Currently supported Assembly-like instructions:

```text
udiv operand
idiv operand
call label
jmp label
ret
exit code
syscall
```

`*` keeps the low bits of the destination width. Signedness does not affect the low result, so there is no separate signed/unsigned multiply form for ordinary assignment math.

`rdx:rax = umul lhs, rhs` lowers to x86-64 `mul` and writes the unsigned 128-bit result across `rdx:rax`. `rdx:rax = imul lhs, rhs` lowers to x86-64 one-operand `imul` and writes the signed 128-bit result across `rdx:rax`. The right operand cannot be an immediate value because x86-64 widened multiply requires a register or memory operand.

`udiv` lowers to x86-64 `div`. `idiv` lowers to x86-64 `idiv`. Both intentionally follow x86-64's one-operand division form. The dividend is implicitly `rdx:rax`, the quotient is written to `rax`, and the remainder is written to `rdx`.

`call label` pushes a return address and jumps to the label. `ret` returns to the caller. Arguments and return values are manual for now; pass them explicitly with registers or memory:

```ss
main: {
  rdi = 2
  rsi = 3
  call add
  // result is now in rax

  exit 0
}

add: {
  rax = rdi + rsi
  ret
}
```

`exit <code>` lowers to the Linux x86-64 `exit` syscall. Exit codes must be between `0` and `255`:

```ss
exit 0
exit 1
```

Negative immediate operands are supported:

```ss
rax = -1
rsp = rsp - 8
rax = rax - 1
```

## Width Rules

Subsea currently rejects mixed-width register operations. These are invalid:

```ss
eax = rax
rax = eax
rax = rax + eax
eax = eax * ax
```

Memory/register operations infer width from the register:

```ss
rax = [addr]  // 64-bit load
[addr] = rax  // 64-bit store
eax = [addr]  // 32-bit load
[addr] = eax  // 32-bit store
```

Ambiguous or unsupported memory moves are rejected:

```ss
[addr] = 5      // no explicit memory width
[rbx] = [rax]   // memory-to-memory assignment is not supported
```

Use explicit memory widths when the compiler cannot infer the width:

```ss
[addr]:u8 = 5
[addr]:i8 = -1
[addr]:u16 = 500
[addr]:i16 = -500
[addr]:u32 = 100000
[addr]:i32 = -100000
[addr]:u64 = 100000
[addr]:i64 = -100000
```

Memory width annotations lower to pointer-sized assembly operands:

```ss
[rax]:u8 = 5
[rax]:i64 = -1
```

```asm
mov byte ptr [rax], 5
mov qword ptr [rax], -1
```

When both a register and an explicit memory width are present, their widths must match:

```ss
[addr]:u64 = rax  // valid
[addr]:u32 = eax  // valid
[addr]:u32 = rax  // invalid
```

Immediate values and integer bindings must fit the destination width:

```ss
al = 255      // valid
al = -1       // valid
al = 256      // invalid
ax = 66000    // invalid

let count:u8 = 255
al = count    // valid
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
rax = [5]
rbx = [rax]
rcx = [label]
rdx = &label
```

Important: numbers inside brackets are memory addresses, not immediate values.

```ss
rax = 5    // immediate value 5
rax = [5]  // memory at address 5
```

Invalid address-of forms:

```ss
rax = &5
rax = &rax
rax = &[rax]
```

Registers can contain addresses, but registers themselves are not addressable memory locations.

## Memory Arithmetic

Memory operands support x86-64-style address expressions:

```ss
rbx = [rax + 8]
rcx = [rbp - 16]
rdx = [rax + rbx + 8]
r8 = [label + 4]
```

Scaled index addressing is also supported:

```ss
rbx = [rax + rcx * 1]
rbx = [rax + rcx * 2]
rbx = [rax + rcx * 4 + 8]
rbx = [label + rcx * 8 - 16]
```

Allowed scales are:

```text
1 2 4 8
```

Only registers can be scaled. These are invalid:

```ss
rbx = [rax + rcx * 3]
rbx = [rax + label * 4]
rbx = [rax + 8 * 4]
```

Nested dereferences and address-of inside memory operands are not supported:

```ss
rbx = [[rax]]
rbx = [&label]
```

## Code Comments

```ss
// inline comment

/*
  multiline
  comment
*/
```
