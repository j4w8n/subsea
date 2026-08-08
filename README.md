# subsea

A more readable, learnable alternative to Assembly, with the same power to directly work with CPU registers and memory. The name is a play on words: subsea is "below C".

The current compiler targets Linux x86-64 by lowering `.ss` source files to assembly, assembling with `as`, and linking with `ld`.

## Assembly-Like Power

Subsea allows direct control over registers, memory, and Linux syscalls while keeping syntax more readable than raw assembly.

```ss
// This program immediately exits with an exit code of 0.
main: {
  rax = 60  // exit syscall number
  rdi = 0   // exit status
  syscall
}
```

> The above is a simple example. You can actually use `exit 0` to do the same thing as those three lines.

## Requirements

Subsea currently requires Linux x86-64 and GNU binutils:

```text
as
ld
```

On many Linux systems these are available through the `binutils` package.

## Example

```ss
main: {
  const message = "Hello World!\n"
  print message

  exit 0
}
```

This prints `Hello World!` and exits the program with status code `0`. Every program must define a top-level `main` label; subsea starts execution there.

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
subsea build -o my_program main.ss
```

`build` writes intermediate assembly and object files to a unique per-build directory under `target/subsea/build-*`. The default executable is written to:

```text
target/subsea/main
```

When `-o` is used, the intermediate files are still written under `target/subsea/build-*`, and the executable is written to the requested output path. `run` uses the same per-build intermediate directory, but removes that directory after the compiled program exits so that build-specific directories don't baloon space.

Use `--timings` or `-t` with `build` to show build times for various phases of the process.

```sh
subsea build -t main.ss
```

## Program Structure

A program starts at a required top-level `main` label:

```ss
main: {
  exit 0
}
```

Labels contain instruction blocks:

```ss
main: {
  const message = "Hello World!\n"
  print message

  exit 0
}
```

Local labels can be used as markers inside a block. They don't start a nested block, so they don't use braces or own the instructions after them. This is also why we choose to not indent a local label. Think of it as a named position in the parent block that code can jump to.

Local labels start with `.` and are scoped to the containing top-level label, so different blocks can reuse names like `.loop:` and `.done:` without collisions:

```ss
main: {
  rax = 3

.loop:
  rax = rax - 1
  jmp .loop
}

other: {
.loop:
  jmp .loop
}
```

Top-level bare labels are allowed when you only need a jump or call target:

```ss
main: {
  jmp skip
}

skip:
```

Labels fall through naturally. If execution reaches a label, it continues through the instructions after that label until a `jmp`, `ret`, `exit`, `syscall` or another control-flow transfer changes execution.

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

## Operands

Instructions work with a small set of operand forms:

```ss
rax       // register
42        // immediate integer
-1        // negative immediate integer
count     // integer binding
[count]   // memory at address count
[rax]     // memory at address in rax
[rax]:u64 // memory at address in rax, with explicit width
&count    // address of label or memory storage
```

Numbers inside brackets are memory addresses, not immediate values:

```ss
rax = 5    // immediate value 5
rax = [5]  // memory at address 5
```

`[5]` is only an address-expression example; dereferencing arbitrary low addresses in a real program will usually crash.

Registers can contain addresses, but registers themselves are not addressable memory locations.

## Bindings

Subsea supports label-local compile-time bindings:

```ss
const message = "Hello World!\n"
print message

const count = 3
print count

const byte_count:u8 = 3
print byte_count
```

Integer bindings can be used as immediate operands:

```ss
const count = 3
rax = count
```

String bindings are printable text bindings, not numeric operands. Use them with `print` or print formatting, not register/memory assignment:

```ss
const message = "Hello World!\n"
print message  // valid
rax = message  // invalid
```

Integer bindings can optionally include a width annotation. Width annotations use the same names as memory widths and are checked when the binding is parsed:

```ss
const byte_count:u8 = 3
const offset:i16 = -8
```

## Stack Variables

`stack` declares label-local mutable storage in the current label's stack frame:

```ss
main: {
  const limit = 5
  stack count:u64 = 0

.loop:
  jmp .done if count u>= limit
  print count
  print "\n"
  count = count + 1
  jmp .loop

.done:
  exit 0
}
```

Stack variables require an explicit width and initializer. Initializers must be integer immediates or integer `const` bindings. A stack variable loads when used as an operand and stores when assigned:

```ss
stack count:u64 = 8
count = 5    // store to stack slot
rax = count  // load from stack slot
```

Stack variables live from label entry to label exit, not from the declaration line. A `stack` declaration inside a loop does not allocate once per iteration.

If a label declares stack variables, Subsea reserves `rbp` for the stack frame in that label. Do not read or write `rbp`, `ebp`, `bp`, or `bpl` manually in a stack-using label.

## Printing

`print "..."` prints literal text directly. `print rax` prints a runtime integer operand as unsigned decimal text. Printing does not add a newline automatically:

```ss
print "Printed directly!\n"

rax = 42
print "rax = "
print rax
print "\n"
```

Formatted printing supports `{}` placeholders with bindings or stack variables:

```ss
const name = "Subsea"
print "Hello, {}\n", name
```

Each `{}` consumes one following identifier argument. The number of placeholders must match the number of arguments. Format specifiers like `{x}` or `{i64}` are not supported yet.

Runtime integer printing is intentionally simple for now. It accepts registers and integer immediates, emits unsigned decimal digits, and does not support signed decimal formatting yet:

```ss
rax = 42
print rax
print "\n"
```

Supported string escapes:

```text
\n
\t
\"
\\
```

Print clobbers:

`print` lowers to the Linux x86-64 `write` syscall. Runtime integer printing also uses `rax` and `rdx` for decimal conversion. Preserve values yourself with `push` and `pop` if you need them after printing:

```text
// print may clobber these registers
rax rdi rsi rdx rcx r11
```

The current convention is that `print` preserves `rbx`, `rbp`, `rsp`, and all registers not listed above. This convention applies to both literal and runtime-integer printing; it may be expanded with explicit save/restore instructions as the runtime grows.

## Assignment Syntax

Subsea uses readable assignment syntax for moving values and doing simple math:

```ss
rax = 10
rax = rax + 5
rax = rax - 1
rbx = count * 2
rdx:rax = rbx u* rcx
rdx:rax = rbx i* rcx
rdx:rax = rbx u/ rcx
rdx:rax = rbx i/ rcx
```

The left side is the destination that changes. Math assignment currently supports `+`, `-`, and low-result `*`. Use `u*` or `i*` with `rdx:rax` when you need the full widened multiply result. Use `u/` or `i/` with `rdx:rax` when you need division; remainder is written to `rdx` and the quotient is written to `rax`.

Widened multiply and divide operands must already be register or memory operands. Numeric literals and integer bindings are immediate values, so put them in explicit registers first:

```ss
r10 = 100
r11 = 10
rdx:rax = r10 u* r11
rdx:rax = r10 u/ r11
```

These are invalid because `100`, `10`, and `count` are immediate values:

```ss
const count = 10
rdx:rax = 100 u* 10
rdx:rax = r10 u/ count
```

## Instructions

```text
call label
jmp label
jmp label if operand == operand
jmp label if operand i< operand
jmp label if operand u< operand
push operand
pop operand
ret
exit code
syscall
```

`call label` pushes a return address and jumps to the label. `ret` returns to the caller. Arguments and return values are manual for now; pass them explicitly with registers or memory:

Subsea functions use a caller-saved convention: callers must preserve values they need across `call`. A callee may modify `rax`, `rcx`, `rdx`, `rdi`, `rsi`, and `r8`-`r11`; `rbx`, `rbp`, and `r12`-`r15` are callee-preserved. `rsp` and `rbp` must be restored before `ret`. Generated stack frames and `print` follow this convention.

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

`jmp label if lhs op rhs` compares two operands and jumps only when the condition is true. Relational comparisons require explicit signedness; plain `<`, `<=`, `>`, and `>=` are rejected. Use `i<`, `i<=`, `i>`, and `i>=` for signed comparisons. Use `u<`, `u<=`, `u>`, and `u>=` for unsigned comparisons. `==` and `!=` are shared because equality does not depend on signedness:

```ss
main: {
  rax = 0
  jmp .zero if rax == 0

.nonzero:
  print "non-zero\n"
  jmp .done

.zero:
  print "zero\n"

.done:
  exit 0
}
```

`ret` emits stack cleanup automatically. `exit` does not need cleanup because the process terminates. Local jumps like `jmp .loop` are allowed in stack-using labels. Jumping from a stack-using label to a different top-level label is rejected. Falling through out of a stack-using label is also rejected.

`push operand` stores a value on the stack and moves `rsp` down. `pop operand` loads a value from the stack and moves `rsp` up:

```ss
push rax
push 10
pop rbx
```

On x86-64, stack operations are pointer-width operations. `push` accepts immediate values, 64-bit registers, and explicitly 64-bit memory operands. `pop` accepts 64-bit registers and explicitly 64-bit memory operands:

```ss
push rax          // valid
push [addr]:u64   // valid
pop rbx           // valid
pop [addr]:u64    // valid

push eax          // invalid: not 64-bit
pop [addr]        // invalid: memory width is ambiguous
pop 10            // invalid: destination cannot be immediate
```

`exit <code>` lowers to the Linux x86-64 `exit` syscall. Exit codes must be between `0` and `255`:

```ss
exit 0
exit 1
```

## Memory And Pointers

Subsea uses `[]` for dereference and `&` for address-of labels.

Top-level `mem` declarations allocate writable memory for the lifetime of the program:

```ss
mem count:u16 = 3
mem buf:u8(128)

main: {
  exit 0
}
```

`mem count:u16 = 3` allocates one writable `u16` memory cell initialized to `3`. `mem buf:u8(128)` allocates 128 zero-initialized writable `u8` cells.

`const`, `mem`, and `stack` answer different storage questions:

```ss
mem total:u16 = 3     // static writable memory

main: {
  const count:u16 = 3  // compile-time constant value
  stack local:u16 = 3  // label-local mutable stack slot
  exit 0
}
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

Examples:

```ss
rax = [count]
rbx = [rax]
rcx = [buf]
rdx = &buf
```

Invalid address-of forms:

```ss
rax = &5
rax = &rax
rax = &[rax]
```

## Memory Arithmetic

Memory operands support x86-64-style address expressions:

```ss
rbx = [rax + 8]
rcx = [rbp - 16]
rdx = [rax + rbx + 8]
r8 = [buf + 4]
```

Scaled index addressing is also supported:

```ss
rbx = [rax + rcx * 1]
rbx = [rax + rcx * 2]
rbx = [rax + rcx * 4 + 8]
rbx = [buf + rcx * 8 - 16]
```

Allowed scales are:

```text
1 2 4 8
```

Only registers can be scaled. These are invalid:

```ss
rbx = [rax + rcx * 3]
rbx = [rax + buf * 4]
rbx = [rax + 8 * 4]
```

Nested dereferences and address-of inside memory operands are not supported:

```ss
rbx = [[rax]]
rbx = [&buf]
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

const count:u8 = 255
al = count    // valid
```

On x86-64, a 64-bit immediate value cannot always be encoded directly into a 64-bit memory destination. If a large value does not fit the direct memory encoding, move it through a 64-bit register first:

```ss
[addr]:u64 = 2147483648  // invalid
rax = 2147483648
[addr]:u64 = rax         // valid
```

## Control-Flow Recipes

These are recipes, not new syntax. Each pattern is built from labels and `jmp`.

While loop:

```ss
main: {
  r8 = 0

.loop:
  jmp .done if r8 u>= 5
  print r8
  print "\n"
  r8 = r8 + 1
  jmp .loop

.done:
  exit 0
}
```

Do-while loop:

```ss
main: {
  r8 = 0

.loop:
  print r8
  print "\n"
  r8 = r8 + 1
  jmp .loop if r8 u< 5

  exit 0
}
```

For-style counted loop:

```ss
main: {
  r8 = 0   // i
  r9 = 10  // limit

.loop:
  jmp .done if r8 u>= r9
  print r8
  print "\n"
  r8 = r8 + 1
  jmp .loop

.done:
  exit 0
}
```

If/else:

```ss
main: {
  rax = 3

  jmp .nonzero if rax != 0
  print "zero\n"
  jmp .done

.nonzero:
  print "non-zero\n"

.done:
  exit 0
}
```

Guard clause or early exit:

```ss
main: {
  rax = 0
  jmp .fail if rax == 0

  print "ok\n"
  exit 0

.fail:
  print "fail\n"
  exit 1
}
```

Break and continue:

```ss
main: {
  r8 = 0

.loop:
  r8 = r8 + 1
  jmp .done if r8 u> 10  // break
  jmp .loop if r8 == 5   // continue
  print r8
  print "\n"
  jmp .loop

.done:
  exit 0
}
```

State machine:

```ss
main: {
  rax = 0
  jmp .state_start

.state_start:
  print "start\n"
  rax = 1
  jmp .state_done if rax == 1
  jmp .state_error

.state_done:
  print "done\n"
  exit 0

.state_error:
  print "error\n"
  exit 1
}
```

Array iteration with scaled addressing:

```ss
mem values:u64(4)

main: {
  [values]:u64 = 10
  [values + 8]:u64 = 20
  [values + 16]:u64 = 30
  [values + 24]:u64 = 40

  r8 = 0
  r9 = 4

.loop:
  jmp .done if r8 u>= r9
  rax = [values + r8 * 8]:u64
  print rax
  print "\n"
  r8 = r8 + 1
  jmp .loop

.done:
  exit 0
}
```

## Code Comments

```ss
// inline comment

/*
  multiline
  comment
*/
```
