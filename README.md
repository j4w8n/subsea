# subsea

A readable, learnable alternative to Assembly, with the same power to directly work with CPU registers, memory, and more. The name is a play on words: subsea is "below C".

Status: working, but early development. If you'd like to play with subsea, you'll need an x86-64 linux system with Rust and `binutils` installed.

## Quickstart

Every program must define a top-level `main` function; subsea starts execution there. Create a `main.ss` file with the following, then run with `subsea run main.ss`:

```ss
main: {
  const message = "Hello from subsea!\n"
  print message

  exit 0
}
```

## Assembly-like Power

Below is an example of using registers and calling the linux kernel. In reality, you can use `exit 0` to do the same thing without the ceremony.

```ss
main: {
  rax = 60
  rdi = 0
  syscall
}
```

## Assignment Syntax

Subsea uses readable assignment syntax for moving values and doing simple math:

```ss
const count = 1

rax = 10
rax = rax + 5
rax = rax - 1
rbx = count * 2
rdx:rax = rbx u* rcx
rdx:rax = rbx i* rcx
rdx:rax = rbx u/ rcx
rdx:rax = rbx i/ rcx
```

- The left side is the destination that changes.
- Math assignment currently supports `+`, `-`, and low-result `*`.
- `i` and `u` prefixes mark whether the operation is for signed or unsigned values.
- Use `u*` or `i*` with `rdx:rax` when you need the full widened multiply result.
- Use `u/` or `i/` with `rdx:rax` when you need division; remainder is written to `rdx` and the quotient is written to `rax`.

Widened multiply and divide operands must already be register or memory operands; immediate values are not supported yet. Numeric literals and integer bindings are immediate values, so put them in explicit registers first:

```ss
r10 = 100
r11 = 10
rdx:rax = r10 u* r11
rdx:rax = r10 u/ r11
```

These are currently invalid because `100`, `10`, and `count` are immediate values:

```ss
const count = 10
rdx:rax = 100 u* 10
rdx:rax = r10 u/ count
```

## Comments

```ss
// inline comment

/*
  multiline
  comment
*/
```

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

- Each `{}` consumes one following identifier argument.
- The number of placeholders must match the number of arguments.
- Format specifiers like `{x}` or `{i64}` are not supported yet.

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

## Functions

These are top-level entities that execute instructions within their code block. Use `call <function>` to call another function, and `ret` to return from a called function. The `main` function is automatically called when a program starts. Other than that, all other functions must be explicitly called in order for their code to run; execution does not "fall through" to the next function, as labels do. Because of this, functions must end with explicit control flow. For example, use `ret`, `exit`, or an equivalent `syscall`.

- `ret` emits generated stack-frame cleanup automatically - unless the stack is manually changed via `push` or `pop`.
- `exit` does not need cleanup because the process terminates. Value must be between `0` and `255`

```ss
main: {
  call helper

  print "Done\n"
  exit 0
}

helper: {
  print "Helping!\n"
  ret
}
```
```bash
Helping!
Done
```

Functions use a mixed caller/callee preservation convention. A callee may freely modify caller-preserved registers `rax`, `rcx`, `rdx`, `rdi`, `rsi`, and `r8`-`r11` without restoring their values before returning. Callers must save those registers themselves if they need their values after `call`. Registers `rbx`, `rbp`, and `r12`-`r15` are callee-preserved, so a callee that changes them must restore their original values before returning.

The stack must also remain balanced across function calls. A callee may move `rsp` while using the stack, but before `ret`, it must undo its own stack changes so `rsp` points at the return address. After `ret`, the caller should see the stack in the same state as before it made the call. Since `rbp` is callee-preserved, any function that uses it as a frame pointer must restore the caller's original `rbp` before returning.

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

## Local Labels

These are named positions that code execution can jump to at any time; like a bookmark. They don't start a nested block or own the instructions after them, which is why it's best practice to not indent the label name.

If execution naturally reaches a label, it continues through the instructions after that label until a `jmp`, `ret`, `exit`, `syscall` or another control-flow transfer changes execution.

Use `jmp .<label>` to jump to a label and continue executing code from that point.

```ss
main: {
  r10 = 5

.loop:
  print r10
  print "\n"

  r10 = r10 - 1
  jmp .loop

  // gotta love those infinite loops!
}
```

For conditional jumps, use `if` and a comparison:
```ss
main: {
  rbx = 5

.loop:
  print rbx
  print "\n"

  jmp .done if rbx == 1

  rbx = rbx - 1
  jmp .loop

.done:
  print "Liftoff!\n"
  exit 0
}
```

```bash
5
4
3
2
1
Liftoff!
```

Labels are scoped to their function, which allows different functions to use the same label names without collisions.

```ss
main: {
.loop:
  jmp .loop
}

other: {
.loop:
  jmp .loop
}
```

You cannot `jmp` from one function's labels to another function's labels.

## Comparison Operators

Comparisons require explicit signedness; plain `<`, `<=`, `>`, and `>=` are rejected. Use `i<`, `i<=`, `i>`, and `i>=` for signed comparisons. Use `u<`, `u<=`, `u>`, and `u>=` for unsigned comparisons.

`==` and `!=` do not depend on signedness.

## Compile-time Bindings

Integer constants can be inlined as operands. Any constant used by `print` is emitted as bytes in `.rodata` and referenced by generated print code.

```ss
const count = 3   // inlined
rax = count

const message = "Hello World!\n"  // referenced
print message
```

String bindings can't be used for register or memory assignment:

```ss
const message = "Hello World!\n"
rax = message  // invalid
```

Integer bindings can optionally include a width annotation. Width annotations use the same names as memory widths and are checked when the binding is parsed:

```ss
const base:u8 = 10
const offset:i16 = -8
```

Floating-point bindings must be given an explicit `f32` or `f64` width, and are supported as compile-time text bindings:

```ss
const ratio:f32 = 1.5
const pi:f64 = 3.14159
print pi  // valid
```

Floating-point literals are valid in typed `const` and top-level `mem` scalar initializers, but they are not immediate runtime operands yet, so assignments like `rax = 1.5` are rejected.

## Memory And Pointers

Top-level `mem` declarations allocate writable memory for the lifetime of the program. Subsea uses `[]` for dereference and `&` for address-of labels.

```ss
mem count:u16 = 3
mem ratio:f64 = 1.5
mem buf:u8(128)

main: {
  exit 0
}
```

- `mem count:u16 = 3` allocates one writable `u16` memory cell initialized to `3`
- `mem buf:u8(128)` allocates 128 zero-initialized writable `u8` cells.
- `mem ratio:f64 = 1.5` allocates one writable `f64` memory cell initialized to `1.5`

Floating-point memory can use XMM registers with explicit `f32` or `f64` memory widths:

```ss
mem single:f32 = 1.5
mem double:f64 = 2.25

main: {
  xmm0 = [single]:f32
  xmm1 = [double]:f64
  [single]:f32 = xmm0
  [double]:f64 = xmm1

  exit 0
}
```

Scalar floating-point arithmetic uses explicit width-prefixed operators:

```ss
mem left:f64 = 1.5
mem right:f64 = 2.25
mem result:f64 = 0.0

main: {
  xmm0 = [left]:f64
  xmm1 = [right]:f64
  xmm0 = xmm0 f64+ xmm1
  xmm0 = xmm0 f64* [right]:f64
  xmm0 = xmm0 f64+ 1.5
  [result]:f64 = xmm0

  exit 0
}
```

- Supported scalar floating-point operators are `f32+`, `f32-`, `f32*`, `f32/`, `f64+`, `f64-`, `f64*`, and `f64/`
- Floating-point arithmetic destinations must be XMM registers.
- Operands must be XMM registers, explicitly annotated floating-point memory operands, `f32`/`f64` const bindings, stack float variables, or float literals matching the operator width.

Floating-point literals and const operands lower to compiler-emitted readonly storage because x86-64 scalar floating-point instructions do not encode decimal float immediates directly:

```ss
mem value:f64 = 2.25

main: {
  const ratio:f64 = 1.5

  xmm0 = [value]:f64
  xmm0 = xmm0 f64+ ratio
  xmm0 = xmm0 f64* 2.0

  exit 0
}
```

Floating-point stack variables use explicit `f32` or `f64` widths and can be loaded/stored with XMM registers:

```ss
main: {
  stack ratio:f64 = 1.5

  xmm0 = ratio
  xmm0 = xmm0 f64+ 2.0
  ratio = xmm0

  exit 0
}
```

Floating-point comparisons use width-prefixed operators and ordered semantics. If either operand is NaN, the jump is not taken:

```ss
main: {
  xmm0 = [left]:f64

  jmp .less if xmm0 f64< 2.0
  exit 0

.less:
  exit 1
}
```

Supported floating-point comparison operators are `f32==`, `f32!=`, `f32<`, `f32<=`, `f32>`, `f32>=`, and the corresponding `f64` forms.

These are not yet supported:

- Runtime float printing

Use `&` to pass the address of `mem` storage:

```ss
mem buf:u8(128)
rsi = &buf
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

Scaled index addressing is also supported for registers, with allowed values of 1, 2, 4, and 8:

```ss
rbx = [rax + rcx * 1]
rbx = [rax + rcx * 2]
rbx = [rax + rcx * 4 + 8]
rbx = [buf + rcx * 8 - 16]
```

Nested dereferences and address-of inside memory operands are not supported:

```ss
rbx = [[rax]]
rbx = [&buf]
```

## Slices

Use `slice <ptr>, <len>` to create a string view over bytes that already exist in memory. It does not copy or allocate.

```ss
mem buf:u8(1024)

main: {
  rax = 0
  rdi = 0
  rsi = &buf
  rdx = 1024
  syscall

  stack input:str = slice &buf, rax
  print input
  exit 0
}
```

## Stack Variables

Use `stack` to declare label-local mutable storage in the current label's stack frame:

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

Scalar stack variables require an explicit width and initializer. Initializers must be integer immediates or integer `const` bindings. A scalar stack variable loads when used as an operand and stores when assigned:

```ss
stack count:u64 = 8
count = 5    // store to stack slot
rax = count  // load from stack slot
```

Stack variables live from label entry to label exit, not from the declaration line. A `stack` declaration inside a loop does not allocate once per iteration.

If a label declares stack variables, Subsea reserves `rbp` for the stack frame in that label. Do not read or write `rbp`, `ebp`, `bp`, or `bpl` manually in a label that uses `stack`.

Stack strings are runtime string slices stored as an address and a byte length. A literal initializer points at compiler-emitted read-only bytes:

```ss
stack message:str = "Hello\n"
print message
```

Access `.ptr` and `.len` to load a stack string's address and byte length as 64-bit operands:

```ss
stack message:str = "Hello\n"
rsi = message.ptr
rdx = message.len
```

## Manual Stack Operations

- `push <operand>` stores a value on the stack and moves `rsp` down.
- `pop <operand>` loads a value from the stack and moves `rsp` up.

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

## Stack Cleanup

- Using `push` or `pop` requires you to keep the stack in balance across function control flow.
  - Every reachable `ret` must have no unmatched manual `push` instructions. A function path that reaches the end of the block without `ret`, `exit`, or an unconditional local `jmp` is also invalid; if that path has unmatched pushes, subsea reports it as unbalanced stack depth first.
  - Local labels must be reached with one consistent stack depth from every path.

```ss
main: {
  push rax
  call helper
  pop rax   // must pop here, or you'll get a stack balance error
  exit 0
}

helper: {
  ret
}
```

## Reading Input

`read stdin, <destination>, <buffer_size>` reads bytes from stdin into writable memory.

- The destination must be address-of top-level memory or a 64-bit register containing an address.
- The buffer size must be an integer immediate, integer `const`, 64-bit register, or 64-bit stack variable.
- `read` leaves the number of bytes read in `rax`
- Negative return values in `rax` are syscall errors.

```ss
mem buf:u8(1024)

main: {
  read stdin, &buf, 1024
  stack input:str = slice &buf, rax
  print input
  exit 0
}
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

Supported XMM registers:

```text
xmm0 xmm1 xmm2 xmm3 xmm4 xmm5 xmm6 xmm7
xmm8 xmm9 xmm10 xmm11 xmm12 xmm13 xmm14 xmm15
```

## Operands

Instructions work with a small set of operand forms:

```ss
rax         // register
xmm0        // XMM register
42          // immediate integer
-1          // negative immediate integer
count       // integer binding
message.ptr // stack string pointer
message.len // stack string length
&count      // address-of identifier or memory storage
[count]     // memory at address count
[rax]       // memory at address in rax
[rax]:u64   // memory at address in rax, with explicit width
[buf + rax]       // memory at identifier plus register offset
[buf + rax * 8]:u64 // scaled register offset; scale must be 1, 2, 4, or 8
```

Numbers inside brackets are memory addresses, not immediate values:

```ss
rax = 5    // immediate value 5
rax = [5]  // memory at address 5
```

`[5]` is only an address-expression example; dereferencing arbitrary low addresses in a real program will usually crash.

Registers can contain addresses, but registers themselves are not addressable memory locations.

XMM registers can be used for floating-point loads, stores, and arithmetic. They cannot be used as memory addresses.

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

## CLI

```sh
subsea run main.ss        // Build and execute the program
subsea build main.ss      // Compile, assemble, and link an executable
subsea emit-asm main.ss   // Compile to x86-64 assembly and print it
```

> `run` exits with the compiled program's exit code.

### build flags

Writes intermediate assembly and object files to a unique per-build directory under `target/subsea/build-*`. The default executable is written to `target/subsea/main`

`-o`: executable is written to the requested output path.

```sh
subsea build -o my_util main.ss
```

`--timings` or `-t`: show build times for various phases of the process.

```sh
subsea build -t main.ss
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
