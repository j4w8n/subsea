# subsea

A readable, learnable alternative to Assembly, with the same power to directly work with CPU registers, memory, and more. The name is a play on words: subsea is "below C".

Status: working, but early development. If you'd like to play with subsea, you'll need an x86-64 linux system with Rust and `binutils` installed.

## Quickstart

Every program must define a top-level `main` function; subsea starts execution there. In emitted assembly, the source-level `main` entry is exposed as the linker-visible entry symbol `_start` by default. Create a `main.ss` file with the following, then run with `subsea run main.ss`:

```ss
main: {
  const message = "Hello from subsea!\n"
  linux.print message

  linux.exit 0
}
```

## Assembly-like Power

Below is an example of using registers and calling the linux kernel. In reality, you can use `linux.exit 0` to do the same thing without the ceremony.

```ss
main: {
  rax = 60
  rdi = 0
  linux.syscall
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
rcx = (rax + 3) * 4
rbx = rax ** 3
rbx = rax u/ 10
rdx = rax u% 10
rdx:rax = rbx u* rcx
rdx:rax = rbx i* rcx
rdx:rax = rbx u/ rcx
rdx:rax = rbx i/ rcx
```

- The left side is the destination that changes.
- Integer math assignment supports `+`, `-`, low-result `*`, signed/unsigned division, signed/unsigned modulo, and power-of.
- Arithmetic expressions support parentheses and normal precedence: `**`, then `*`/division/modulo, then `+`/`-`, then shifts, then bitwise `&`, `^`, and `|`.
- Power uses `**`. Runtime exponents are supported for integer operands; narrower exponent registers or memory operands are zero-extended before the loop, so `rax = rbx ** cl` treats `cl` as an unsigned 8-bit exponent.
- Power currently requires a 64-bit integer destination and 64-bit base. Narrow exponents are allowed because they only control the loop count; narrow base/result forms like `eax = ebx ** cl` are not supported yet. Negative immediate exponents are rejected. Results use normal integer wrapping/truncation behavior from repeated `imul` operations.
- `i` and `u` prefixes mark whether the operation is for signed or unsigned values.
- Division must use `i/` or `u/`; plain `/` is rejected.
- Modulo must use `i%` or `u%`; plain `%` is rejected.
- Use `u*` or `i*` with `rdx:rax` when you need the full widened multiply result.
- Use `u/` or `i/` with `rdx:rax` when you need division; remainder is written to `rdx` and the quotient is written to `rax`.

Widened multiply and divide write their hardware result to `rdx:rax`; other register pairs are not accepted. Immediate operands are allowed.

```ss
const count = 10
rdx:rax = 100 u* count
rdx:rax = r10 u/ 10
```

Arithmetic expression lowering may also use `r10` or `r11` as scratch registers. Power-of uses `r10` for the base and `r11` for the exponent. Do not rely on `r10` or `r11` being preserved across arithmetic expressions, power-of, low-result division/modulo, or widened multiply/divide with immediate or clobbered right operands.

## Compile-time Bindings

Integer constants can be inlined as operands. Any constant used by `linux.print` is emitted as bytes in `.rodata` and referenced by generated print code.

```ss
const count = 3   // inlined
rax = count

const message = "Hello World!\n"  // referenced
linux.print message
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
linux.print pi  // valid
```

Floating-point literals are valid in typed `const` and top-level `mem` scalar initializers. They can also be used as runtime operands when a floating-point width is supplied by context, such as `xmm0 = xmm0 f64+ 1.5`; assignments like `rax = 1.5` are rejected.

## Bitwise Operations

Subsea supports common integer bitwise operations with assignment syntax:

```ss
rax = rbx & rcx   // and
rax = rbx | rcx   // or
rax = rbx ^ rcx   // xor
rax = ~rbx        // not
rax = rbx << 3    // shift left
rax = rbx >> 3    // logical shift right
rax = rbx i>> 3   // arithmetic shift right
```

Shift counts must be immediate values or `cl`, matching x86-64 encoding rules:

```ss
rax = rax << 1   // valid
rax = rax << cl  // valid
rax = rax << rcx // invalid; use cl
```

Bitwise operations are integer-only. They do not support XMM registers or floating-point memory operands.

## Comparison Operators

Integer comparisons require explicit signedness. Use `i<`, `i<=`, `i>`, and `i>=` for signed comparisons. Use `u<`, `u<=`, `u>`, and `u>=` for unsigned comparisons.

`==` and `!=` do not depend on signedness.

Plain `<`, `<=`, `>`, and `>=` are allowed for floating-point comparisons only when a `f32` or `f64` width can be inferred from an operand. Otherwise, use width-prefixed float operators such as `f64<`.

## Condition Results And Conditional Assignment

Conditions can be used in more places than jumps. Assigning a condition stores `1` when the condition is true and `0` when it is false:

```ss
rax = rdi i< rsi
al = rbx == 0
```

Conditions can also guard an assignment. The destination is changed only when the condition is true:

```ss
rax = rbx if rcx == 0
count = count + 1 if count u< 10
```

Use bitwise-and conditions to test whether masked bits are clear or set. The number after `&` is a mask: an immediate integer literal used to choose which bits to inspect.

```ss
rax = 5

jmp .has_bit if rax & 4 != 0
al = rax & 4 != 0
rbx = 99 if rax & 4 != 0

// true when the low 4 bits are all zero, which means rax is 16-byte aligned
jmp .aligned if rax & 15 == 0
```

The comparison applies to the result of `lhs & mask`. For example, `rax & 15 == 0` means `(rax & 15) == 0`, not `15 == 0`.

Bitwise-and conditions must compare against `0` with `==` or `!=`. Subsea lowers these conditions to x86-64 `test` internally.

## Comments

```ss
// inline comment

/*
  multiline
  comment
*/
```

## Printing

`linux.print "..."` prints literal text directly. `linux.print rax` prints a runtime integer operand as unsigned decimal text. Printing does not add a newline automatically:

```ss
linux.print "Printed directly!\n"

rax = 42
linux.print "rax = "
linux.print rax
linux.print "\n"
```

Formatted printing supports `{}` placeholders with bindings or stack variables:

```ss
const name = "Subsea"
linux.print "Hello, {}\n", name
```

- Each `{}` consumes one following identifier argument.
- The number of placeholders must match the number of arguments.
- Format specifiers like `{x}` or `{i64}` are not supported yet.

Runtime integer printing is intentionally simple for now. It accepts registers and integer immediates, emits unsigned decimal digits, and does not support signed decimal formatting yet:

```ss
rax = 42
linux.print rax
linux.print "\n"
```

Supported string escapes:

```text
\n
\t
\"
\\
```

Print clobbers:

`linux.print` lowers to the Linux x86-64 `write` syscall. Runtime integer printing also uses `rax` and `rdx` for decimal conversion. Preserve values yourself with `push` and `pop` if you need them after printing:

```text
// linux.print may clobber these registers
rax rdi rsi rdx rcx r11
```

The current convention is that `linux.print` preserves `rbx`, `rbp`, `rsp`, and all registers not listed above. This convention applies to both literal and runtime-integer printing; it may be expanded with explicit save/restore instructions as the runtime grows.

## Reading Input

`linux.read(stdin, <destination>, <buffer_size>)` reads bytes from stdin into writable memory.

- The destination must be address-of top-level memory or a 64-bit register containing an address.
- The buffer size must be an integer immediate, integer `const`, 64-bit register, or 64-bit stack variable.
- `read` leaves the number of bytes read in `rax`
- Negative return values in `rax` are syscall errors.

```ss
mem buf:u8(1024)

main: {
  linux.read(stdin, &buf, 1024)
  stack input:str = slice(&buf, rax)
  linux.print input
  linux.exit 0
}
```

## Functions

These are top-level entities that execute instructions within their code block. Use `call <function>` to call another function, and `ret` to return from a called function. The `main` function is automatically called when a program starts. Other than that, all other functions must be explicitly called in order for their code to run; execution does not "fall through" to the next function, as labels do. Because of this, functions must end with explicit control flow. For example, use `ret`, `linux.exit`, or an equivalent `linux.syscall`.

- `ret` emits generated stack-frame cleanup automatically - unless the stack is manually changed via `push` or `pop`.
- `linux.exit` does not need cleanup because the process terminates. Value must be between `0` and `255`
- `nop` emits a no-operation instruction. It is useful for padding, patch space, and low-level debugging.

```ss
main: {
  call helper

  linux.print "Done\n"
  linux.exit 0
}

helper: {
  linux.print "Helping!\n"
  ret
}
```
```bash
Helping!
Done
```

Functions use a mixed caller/callee preservation convention. A callee may freely modify caller-preserved registers `rax`, `rcx`, `rdx`, `rdi`, `rsi`, and `r8`-`r11` without restoring their values before returning. Callers must save those registers themselves if they need their values after `call`. Registers `rbx`, `rbp`, and `r12`-`r15` are callee-preserved, so a callee that changes them must restore their original values before returning.

```ss
main: {
  rdi = 2
  rsi = 3
  call add
  // result is now in rax

  linux.exit 0
}

add: {
  rax = rdi + rsi
  ret
}
```

The stack must also remain balanced across function calls. A callee may move `rsp` while using the stack, but before `ret`, it must undo its own stack changes so `rsp` points at the return address. After `ret`, the caller should see the stack in the same state as before it made the call. Since `rbp` is callee-preserved, any function that uses it as a frame pointer must restore the caller's original `rbp` before returning.

Calls may also target a 64-bit register or memory operand containing a function address (more on `mem` in [Memory And Pointers](#memory-and-pointers)):

```ss
mem callback:ptr = addr handler

main: {
  rax = [callback]
  call rax
  linux.exit 0
}

handler: {
  ret
}
```

Indirect call targets must be 64-bit integer registers or memory operands. They use the same calling convention as symbolic calls.

## Imports

Reusable functions can be imported explicitly from another `.ss` file. Import paths are relative to the file that contains the import. Imported files must mark public functions with `export`; private helper functions remain callable inside the imported file but cannot be imported directly.

```ss
import debug_write from "lib/qemu_debug.ss"

main: {
  const message = "Subsea\n"
  rsi = message.ptr
  rdx = message.len
  call debug_write

  linux.exit 0
}
```

The imported file exports functions with `export name: { ... }`:

```ss
export debug_write: {
.loop:
  jmp .done if rdx == 0

  al = [rsi]:u8
  x86 "out 0xe9, al"

  rsi = rsi + 1
  rdx = rdx - 1
  jmp .loop

.done:
  ret
}
```

Imports are intentionally narrow: only explicitly listed exported functions can be imported. Memory, data blocks, constants, and private helper functions are not importable API surface yet.

## Local Labels

These are named positions that code execution can jump to at any time; like a bookmark. They don't start a nested block or own the instructions after them, which is why it's best practice to not indent the label name.

If execution naturally reaches a label, it continues through the instructions after that label until a `jmp`, `ret`, `linux.exit`, `linux.syscall` or another control-flow transfer changes execution.

Use `jmp .<label>` to jump to a label and continue executing code from that point.

```ss
main: {
  r10 = 5

.loop:
  linux.print r10
  linux.print "\n"

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
  linux.print rbx
  linux.print "\n"

  jmp .done if rbx == 1

  rbx = rbx - 1
  jmp .loop

.done:
  linux.print "Liftoff!\n"
  linux.exit 0
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

Symbolic `jmp .<label>` cannot jump from one function's local labels to another function's local labels.

Jumps may also target a 64-bit register or memory operand containing an address (more on `mem` in the next section). So, it's possible to jump to another function. This is useful for dispatch tables and low-level runtimes:

```ss
mem handlers:ptr = [addr state_zero, addr state_one]

main: {
  rax = 1
  jmp handlers[rax * 8]:ptr
}

state_zero: {
  linux.exit 0
}

state_one: {
  linux.exit 1
}
```

Conditional indirect jumps are supported and lower to a normal conditional branch around an indirect `jmp`:

```ss
jmp rax if rcx == 0
```

An indirect `jmp` transfers control to an unknown destination, so the manual push/pop stack depth must be balanced at the jump site (see [Stack Cleanup](#stack-cleanup)).

## Memory And Pointers

Top-level `mem` declarations allocate writable memory for the lifetime of the program. Bracketed memory operands load from or store through memory, while `&` computes an address without reading memory.

Memory operands rooted at declared `mem` storage infer the declaration width:

```ss
mem single:f32 = 1.5
mem double:f64 = 2.25

main: {
  xmm0 = [single]
  xmm1 = [double]
  [single] = xmm0
  [double] = xmm1

  linux.exit 0
}
```

The inferred width is the access width; address arithmetic remains byte-based. `mem nums:u64(8)` makes `nums[8]` a `u64` access to the second element.

Use `&` to pass the address of `mem` storage:

```ss
mem buf:u8(128)
rsi = &buf
```

Invalid address-of forms:

```ss
rax = &5
rax = &rax
```

`&[...]` is valid and computes a raw address expression without loading memory. See [Memory Arithmetic](#memory-arithmetic).

```ss
mem count:u16 = 3
mem ratio:f64 = 1.5
mem buf:u8(128)
mem greeting:u8 = "hello\n"   // stored as bytes
mem values:u16 = [1, 2, 3]    // array initialization
mem fill:u8 = repeat(4, 0xff)
mem callback:ptr = addr main  / store's `main`'s address

main: {
  linux.exit 0
}
```

- `mem count:u16 = 3` allocates one writable `u16` memory cell initialized to `3`
- `mem buf:u8(128)` allocates 128 zero-initialized writable `u8` cells.
- `mem ratio:f64 = 1.5` allocates one writable `f64` memory cell initialized to `1.5`
- `mem greeting:u8 = "hello\n"` allocates writable bytes initialized from the string literal. String memory initializers require `u8` width and do not add an implicit NUL terminator.
- `mem values:u16 = [1, 2, 3]` allocates initialized writable arrays. Each value is range-checked against the declared width.
- `mem fill:u8 = repeat(4, 0xff)` emits four initialized `u8` values. The `repeat` form uses `repeat(<count>, <value>)`.
- `mem callback:ptr = addr main` allocates one pointer-sized address constant. On x86-64, `ptr` is 8 bytes and emits `.quad` data.

Pointer-sized arrays are useful for static dispatch tables and address lists:

```ss
mem handlers:ptr = [addr init, addr update, addr shutdown]
```

`ptr` memory initializers currently require `addr <symbol>` values; integer pointer literals are intentionally not accepted yet.

## Slices

Use `slice(<ptr>, <len>)` to create a string view over bytes that already exist in memory. It does not copy or allocate.

```ss
mem buf:u8(1024)

main: {
  rax = 0
  rdi = 0
  rsi = &buf
  rdx = 1024
  linux.syscall

  stack input:str = slice(&buf, rax)
  linux.print input
  linux.exit 0
}
```

## Memory Arithmetic

Declared memory can be indexed with byte offsets. The access width is inferred from the `mem` declaration unless you add an explicit width:

```ss
mem values:u64(4)
mem bytes:u8(128)

values[0] = 10
values[8] = 20
rax = values[8]       // u64 load

bytes[rax] = 72       // u8 store
al = bytes[rax]       // u8 load
```

Use `&name[offset]` to compute the address of indexed memory without loading or storing through it:

```ss
rsi = &bytes[rax]
stack text:str = slice(&bytes[10], 20)
```

The difference is whether the expression reads memory:

```ss
al = bytes[rax]   // load one byte from bytes + rax
rsi = &bytes[rax] // compute bytes + rax; do not load memory
```

Index expressions can use scaled registers, with scale values of 1, 2, 4, and 8:

```ss
rax = values[r8 * 8]
rsi = &values[r8 * 8]
```

Raw memory operands also support x86-64-style address expressions. Use this form when the base address is already in a register:

```ss
rbx = [rax + 8]
rcx = [rbp - 16]
rdx = [rax + rbx + 8]
```

Scaled index addressing is also supported in raw memory operands. Brackets without `&` mean load from the computed address:

```ss
rbx = [rax + rcx * 1]
rbx = [rax + rcx * 2]
rbx = [rax + rcx * 4 + 8]
```

Use `&[...]` to compute a raw register-based address expression without reading memory. On x86-64 this lowers to `lea`:

```ss
rbx = [rax + rcx * 4 + 8]  // load from memory at rax + rcx * 4 + 8
rbx = &[rax + rcx * 4 + 8] // compute rax + rcx * 4 + 8; do not load memory
```

Nested dereferences and address-of inside memory operands are not supported:

```ss
rbx = [[rax]]
rbx = [&buf]
```

## Floating Point

Scalar floating-point arithmetic uses explicit width-prefixed operators:

```ss
mem left:f64 = 1.5
mem right:f64 = 2.25
mem result:f64 = 0.0

main: {
  xmm0 = [left]
  xmm1 = [right]
  xmm2 = xmm0
  xmm0 = xmm0 f64+ xmm1
  xmm0 = xmm0 f64* [right]
  xmm0 = xmm0 f64+ 1.5
  xmm3 = rax::f64
  [result] = xmm0

  linux.exit 0
}
```

- Supported scalar floating-point operators are `f32+`, `f32-`, `f32*`, `f32/`, `f64+`, `f64-`, `f64*`, and `f64/`
- Floating-point arithmetic destinations must be XMM registers.
- Operands must be XMM registers, floating-point memory operands, `f32`/`f64` const bindings, stack float variables, or float literals matching the operator width. Memory widths may be explicit or inferred from a declared `mem` base.
- XMM register-to-register moves are supported with normal assignment syntax, such as `xmm2 = xmm0`.

Use `::f32` or `::f64` to cast 32-bit or 64-bit integer register/memory operands into XMM registers:

```ss
xmm0 = eax::f32
xmm1 = rax::f64
```

Use `::i8`, `::i16`, `::i32`, `::i64`, or the corresponding unsigned widths to cast typed floating-point memory operands into integer registers. Float-to-int casts use x86 truncating conversion semantics:

```ss
mem ratio:f64 = 1.5
rax = [ratio]::i64
ecx = [ratio]::i32
```

Casting directly from an XMM register to an integer register is not supported yet because XMM registers do not carry an `f32` or `f64` source width in the syntax.

Floating-point literals and const operands lower to compiler-emitted readonly storage because x86-64 scalar floating-point instructions do not encode decimal float immediates directly. Plain `+`, `-`, and `*` can be used for floating-point arithmetic when an operand supplies an unambiguous `f32` or `f64` width. Use width-prefixed operators when both operands are ambiguous, such as XMM register-to-register or XMM register-to-float-literal arithmetic:

```ss
mem value:f64 = 2.25

main: {
  const ratio:f64 = 1.5

  xmm0 = [value]
  xmm0 = xmm0 + ratio
  xmm0 = xmm0 f64* 2.0

  linux.exit 0
}
```

Floating-point stack variables use explicit `f32` or `f64` widths and can be loaded/stored with XMM registers:

```ss
main: {
  stack ratio:f64 = 1.5

  xmm0 = ratio
  xmm0 = xmm0 f64+ 2.0
  ratio = xmm0

  linux.exit 0
}
```

Floating-point comparisons use ordered semantics. If either operand is NaN, the jump is not taken. Plain comparison operators work when a float width can be inferred from an operand:

```ss
mem left:f64 = 1.5

main: {
  xmm0 = [left]

  jmp .less if xmm0 < [left]
  linux.exit 0

.less:
  linux.exit 1
}
```

Use width-prefixed comparison operators when both operands are ambiguous, such as XMM register-to-register comparisons or XMM register-to-float-literal comparisons:

```ss
jmp .less if xmm0 f64< xmm1
jmp .less if xmm0 f64< 2.0
```

Supported floating-point comparison operators are `f32==`, `f32!=`, `f32<`, `f32<=`, `f32>`, `f32>=`, and the corresponding `f64` forms.

These are not yet supported:

- Runtime float printing

## Stack Variables

Use `stack` to declare label-local mutable storage in the current label's stack frame:

```ss
main: {
  const limit = 5
  stack count:u64 = 0

.loop:
  jmp .done if count u>= limit

  linux.print count
  linux.print "\n"

  count = count + 1
  jmp .loop

.done:
  linux.exit 0
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
linux.print message
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
  - Every reachable `ret` must have no unmatched manual `push` instructions. A function path that reaches the end of the block without `ret`, `linux.exit`, or an unconditional local `jmp` is also invalid; if that path has unmatched pushes, subsea reports it as unbalanced stack depth first.
  - Local labels must be reached with one consistent stack depth from every path.

```ss
main: {
  push rax
  call helper
  pop rax   // must pop here, or you'll get a stack balance error
  linux.exit 0
}

helper: {
  ret
}
```

## Typed Intrinsic Calls

Typed intrinsic calls use call-style syntax with an explicit result type. They are whole right-hand-side assignment values; they are not expression terms yet.

```ss
rax = min(rbx, rcx):i64
rax = max(rbx, 10):u64
xmm0 = sqrt(xmm1):f64
xmm1 = min(xmm1, 0.0):f32
xmm2 = round(xmm2):f64
xmm3 = floor(1.75):f32
xmm4 = ceil(xmm5):f64
xmm6 = trunc(xmm7):f32
```

- Supported typed intrinsics are `min`, `max`, `sqrt`, `round`, `floor`, `ceil`, and `trunc`.
- `min` and `max` support scalar integer widths `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, and `u64` with signedness taken from the width.
- Integer `min` and `max` destinations must be integer registers. `i8` and `u8` are lowered with branches because x86-64 does not have 8-bit conditional moves.
- `min`, `max`, `sqrt`, `round`, `floor`, `ceil`, and `trunc` support scalar floating widths `f32` and `f64`; floating-point destinations must be XMM registers.
- Integer `sqrt` is not implemented yet; `sqrt(...):i64` and other integer widths are rejected.
- Integer result rounding is not implemented yet; `round(...):i64` and other integer widths are rejected.
- Floating-point rounding emits SSE4.1 `roundss` or `roundsd`: `round` uses nearest, `floor` rounds down, `ceil` rounds up, and `trunc` rounds toward zero.
- Floating-point intrinsic operands can be XMM registers, floating-point memory operands, `f32`/`f64` const bindings, stack float variables, or float literals matching the intrinsic width.

## Freestanding And Raw X86

`x86 "..."` emits one raw x86-64 assembly instruction. It is mainly useful for explicit architecture interop in freestanding code.

Freestanding halt loop:

```ss
main: {
.hang:
  x86 "hlt"
  jmp .hang
}
```

Port I/O can be written as raw x86 assembly. The examples below use byte-sized `in`/`out`: `al` is the data register, and the port must be an immediate `0..255` or `dx`.

```ss
main: {
  al = 72
  x86 "out 0x80, al"

  x86 "in al, dx"

.hang:
  x86 "hlt"
  jmp .hang
}
```

Port I/O is mainly useful in freestanding code for hardware interaction, such as serial output after UART initialization.

For simple QEMU smoke tests, the debug console can be connected to port `0xe9`. The `examples/lib/qemu_debug.ss` helper uses a local calling convention: the caller passes the string pointer in `rsi` and the byte length in `rdx`; `debug_write` then loops over the bytes and emits each one with raw x86 port I/O.

```ss
import debug_write from "../lib/qemu_debug.ss"

main: {
  const message = "Subsea\n"

  rsi = message.ptr
  rdx = message.len
  call debug_write

.hang:
  x86 "hlt"
  jmp .hang
}
```

This keeps the hardware boundary explicit while avoiding manual ASCII byte assignments for every character.

## Static Data Blocks

Use top-level `data` blocks for explicit static metadata layout in named object sections. This is useful for freestanding metadata, firmware tables, linker-collected registries, and boot protocol records:

```ss
data request section ".requests" align 8 export keep {
  u64 0xc7b1dd30df4c8b88
  u64 0x0a82e883a194f07b
  u64 0
  u64 0
  addr response
  zero 16

response:
  u64 0
}
```

Supported data items are fixed-width integer scalars (`u8`, `u16`, `u32`, `u64`, `i8`, `i16`, `i32`, `i64`), `addr <symbol>` for an address-sized symbol reference, `zero <bytes>` for zero-filled bytes, and labels inside the block. On x86-64, `addr` emits an 8-byte relocation.

- `section` selects the exact output section name.
- `align` must be a non-zero power of two.
- `export` makes the data block symbol global.
- On x86-64 ELF, `keep` emits a retained section using the GNU `R` section flag. Linker scripts can still use `KEEP(*(.section_name))` for compatibility with linkers that do not honor retained sections.

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
al::zx      // zero-extend an integer source operand
al::sx      // sign-extend an integer source operand
xmm0        // XMM register
42          // immediate integer
-1          // negative immediate integer
count       // integer binding
message.ptr // stack string pointer
message.len // stack string length
&count      // address-of identifier or memory storage
values[rax] // indexed memory rooted at declared storage
&values[rax] // address of indexed memory
[count]     // memory at address count
[rax]       // memory at address in rax
[rax]:u64   // memory at address in rax, with explicit width
[rax + rbx * 8]:u64 // raw memory with scaled register offset
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

Same-width integer moves work directly. Moving from a wider integer register into a narrower destination truncates to the low bits:

```ss
rax = 257
al = rax   // al gets the low 8 bits: 1
ax = rax   // ax gets the low 16 bits: 257
```

Moving from a narrower source into a wider destination requires an explicit width conversion. Use `::zx` to zero-extend and `::sx` to sign-extend:

```ss
rax = al::zx   // zero-extend al into rax
rax = al::sx   // sign-extend al into rax
rax = eax::zx  // zero-extend eax into rax
rax = eax::sx  // sign-extend eax into rax
```

Implicit widening is invalid because Subsea needs to know how to fill the new upper bits:

```ss
rax = al   // invalid; use al::zx or al::sx
```

Other mixed-width arithmetic is still rejected:

```ss
rax = rax + eax
eax = eax * ax
```

Memory/register operations can infer width from the register when the memory address has no declared storage base:

```ss
rax = [addr]  // 64-bit load
[addr] = rax  // 64-bit store
eax = [addr]  // 32-bit load
[addr] = eax  // 32-bit store
```

Memory operands rooted at declared `mem` storage infer the declared width instead:

```ss
mem buf:u8(8)
mem count:u64 = 0

[buf] = 72       // u8 store
[buf + 1] = 105  // u8 store; offset is still byte-based
al = [buf]       // valid u8 load
rax = [buf]      // invalid: u8 source with u64 destination
[count] = 3      // u64 store
```

Subsea does not track value types inside registers. A register used as an address does not provide a memory access width:

```ss
rax = &buf
[rax] = 72      // invalid: no memory width
[rax]:u8 = 72   // valid
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

### targets

The default target is `x86_64`, which means x86-64 Linux userland. This target supports Linux helpers such as `linux.print`, `linux.read(stdin, ...)`, and `linux.exit`.

Use `x86_64-free` for freestanding x86-64 assembly. This target still emits x86-64 instructions, but it rejects Linux-only helpers because there may be no Linux process, stdout, stdin, or process exit:

```sh
subsea emit-asm --target x86_64-free kernel.ss
subsea emit-asm -t x86_64-free kernel.ss
subsea build --target x86_64-free -o kernel.o kernel.ss
subsea build --target x86_64-free --linker-script kernel.ld -o kernel.elf kernel.ss
subsea build --target x86_64-free -T kernel.ld --format binary -o kernel.bin kernel.ss
```

By default, Subsea emits source-level `main` as the linker-visible symbol `_start`. Freestanding mode can override that symbol with `--entry`:

```sh
subsea emit-asm -t x86_64-free --entry kernel_entry kernel.ss
```

For `x86_64-free`, `build` writes an object file instead of linking an executable by default. This keeps freestanding output composable with external boot code, linker scripts, and custom build systems. If `-o` is omitted, the object file is written to `target/subsea/main.o`.

Pass `--linker-script` or `-T` to link the freestanding object with `ld -T` and write a freestanding executable instead:

```sh
subsea build -t x86_64-free -T kernel.ld -o kernel.elf kernel.ss
```

Freestanding linker-script builds pass `-m elf_x86_64` to the linker so output does not depend on the host linker's default emulation. Use `--linker` to select a different linker program, such as `ld.lld` or `x86_64-elf-ld`.

Raw binary output is available for linked freestanding builds with `--format binary`. It links an ELF first, then runs `objcopy -O binary`:

```sh
subsea build -t x86_64-free -T kernel.ld --format binary -o kernel.bin kernel.ss
```

Freestanding support is early. Raw binaries are not bootable by themselves; they still need a boot sector, firmware header, bootloader protocol metadata, or an external bootloader before a QEMU smoke test is meaningful.

See `examples/freestanding` for object/ELF/raw-binary examples, and `examples/limine` for a minimal Limine-oriented kernel ELF with request metadata declared in `.ss` data blocks. The Limine example documents a manual QEMU debug-port smoke test; it is not run by `cargo test` because it depends on external Limine binaries, ISO creation tools, and QEMU.

### Limine/QEMU smoke test

From `examples/limine`, build the kernel into the ISO staging tree:

```sh
subsea build -t x86_64-free -T kernel.ld -o iso_root/boot/kernel.elf kernel.ss
```

Build a Limine bootable ISO from `iso_root`:

```sh
xorriso -as mkisofs -R -r -J \
  -b boot/limine-bios-cd.bin \
  -no-emul-boot \
  -boot-load-size 4 \
  -boot-info-table \
  -hfsplus \
  -apm-block-size 2048 \
  --efi-boot boot/limine-uefi-cd.bin \
  -efi-boot-part \
  --efi-boot-image \
  --protective-msdos-label \
  iso_root \
  -o subsea.iso
```

Install Limine BIOS stages into the ISO:

```sh
limine bios-install subsea.iso
```

Run it in QEMU with the debug console connected to port `0xe9`:

```sh
qemu-system-x86_64 -M q35 -m 256M -cdrom subsea.iso -debugcon stdio -global isa-debugcon.iobase=0xe9
```

If `kernel.ss` writes bytes with `x86 "out 0xe9, al"`, they appear in the terminal where QEMU is running. The QEMU display window may stay blank because this example does not write to the framebuffer.

### build flags

Writes intermediate assembly and object files to a unique per-build directory under `target/subsea/build-*`. The default executable is written to `target/subsea/main`

`-o`: output is written to the requested path. For `x86_64`, this is a linked executable. For `x86_64-free`, this is an object file unless `--linker-script`/`-T` is provided.

```sh
subsea build -o my_util main.ss
subsea build -t x86_64-free -o kernel.o kernel.ss
```

`--linker-script` or `-T`: link `x86_64-free` output with the requested linker script. This writes a freestanding executable instead of an object file.

```sh
subsea build -t x86_64-free --linker-script kernel.ld -o kernel.elf kernel.ss
subsea build -t x86_64-free -T kernel.ld -o kernel.elf kernel.ss
```

`--format`: select the linked freestanding output format. Supported values are `elf` and `binary`. `binary` requires `--linker-script`/`-T`.

```sh
subsea build -t x86_64-free -T kernel.ld --format elf -o kernel.elf kernel.ss
subsea build -t x86_64-free -T kernel.ld --format binary -o kernel.bin kernel.ss
```

`--linker`: select the linker program for `x86_64-free` linker-script builds. The default is `ld`.

```sh
subsea build -t x86_64-free --linker ld.lld -T kernel.ld -o kernel.elf kernel.ss
```

`--link-input`: add an extra object file to an `x86_64-free` linker-script build. This flag can be repeated.

```sh
subsea build -t x86_64-free -T kernel.ld --link-input boot.o --link-input tables.o -o kernel.elf kernel.ss
```

`--target` or `-t`: select a target. Supported targets are `x86_64` and `x86_64-free`.

```sh
subsea build -t x86_64-free kernel.ss
```

`--entry`: select the linker-visible entry symbol for `x86_64-free`. The source program still uses `main`; codegen emits that entry block with the requested symbol.

```sh
subsea build -t x86_64-free --entry kernel_entry kernel.ss
```

`--timings`: show build times for various phases of the process.

```sh
subsea build --timings main.ss
```

## Control-Flow Recipes

These are recipes, not new syntax. Each pattern is built from labels and `jmp`.

While loop:

```ss
main: {
  r8 = 0

.loop:
  jmp .done if r8 u>= 5
  linux.print r8
  linux.print "\n"
  r8 = r8 + 1
  jmp .loop

.done:
  linux.exit 0
}
```

Do-while loop:

```ss
main: {
  r8 = 0

.loop:
  linux.print r8
  linux.print "\n"
  r8 = r8 + 1
  jmp .loop if r8 u< 5

  linux.exit 0
}
```

For-style counted loop:

```ss
main: {
  r8 = 0   // i
  r9 = 10  // limit

.loop:
  jmp .done if r8 u>= r9
  linux.print r8
  linux.print "\n"
  r8 = r8 + 1
  jmp .loop

.done:
  linux.exit 0
}
```

If/else:

```ss
main: {
  rax = 3

  jmp .nonzero if rax != 0
  linux.print "zero\n"
  jmp .done

.nonzero:
  linux.print "non-zero\n"

.done:
  linux.exit 0
}
```

Guard clause or early linux.exit:

```ss
main: {
  rax = 0
  jmp .fail if rax == 0

  linux.print "ok\n"
  linux.exit 0

.fail:
  linux.print "fail\n"
  linux.exit 1
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
  linux.print r8
  linux.print "\n"
  jmp .loop

.done:
  linux.exit 0
}
```

State machine:

```ss
main: {
  rax = 0
  jmp .state_start

.state_start:
  linux.print "start\n"
  rax = 1
  jmp .state_done if rax == 1
  jmp .state_error

.state_done:
  linux.print "done\n"
  linux.exit 0

.state_error:
  linux.print "error\n"
  linux.exit 1
}
```

Array iteration with scaled addressing:

```ss
mem values:u64(4)

main: {
  values[0] = 10
  values[8] = 20
  values[16] = 30
  values[24] = 40

  r8 = 0
  r9 = 4

.loop:
  jmp .done if r8 u>= r9
  rax = values[r8 * 8]
  linux.print rax
  linux.print "\n"
  r8 = r8 + 1
  jmp .loop

.done:
  linux.exit 0
}
```
