# subsea

A readable, learnable alternative to Assembly, with the same power to directly work with CPU registers, memory, and more. The name is a play on words: subsea is "below C".

Checkout our Subsea vs Assembly comparisons in the [examples](examples/x86-vs-subsea/README.md)

All examples currently use x86_64 registers, but aarch64 registers are also supported.

Status: working, but early development. If you'd like to play with subsea, you'll need an x86-64 or aarch64 linux system with Rust and appropriate "as" and "ld" binaries.

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

Register pairs can also be used for 128-bit add-with-carry and subtract-with-borrow. Pair operands use `high:low` order, matching `rdx:rax`:

```ss
// add with carry
rdx:rax = rdx:rax + rcx:rbx

// subtract with borrow
rdx:rax = rdx:rax - rcx:rbx
```

The low halves are added or subtracted first, then the high halves consume the carry or borrow:

```asm
add rax, rbx
adc rdx, rcx

sub rax, rbx
sbb rdx, rcx
```

For now, pair add/sub requires 64-bit integer registers. The destination pair must match the left operand pair so every changed register is visible in the assignment. Destination high/low registers must be different, and the right high register cannot overlap the destination low register because the low operation runs first.

Arithmetic expression lowering may use `r10`, `r11`, `r8`, or `r9` as scratch registers, preferring `r10` and `r11` when available. Floating-point intrinsic assignments to memory use `xmm15` as an explicit scratch register. Power-of uses `r10` for the base and `r11` for the exponent. Do not rely on these scratch registers being preserved across arithmetic expressions, power-of, low-result division/modulo, widened multiply/divide with immediate or clobbered right operands, or floating-point intrinsics assigned to memory.

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

Valid integer comparisons:

```ss
rax = rdi i< rsi   // signed less-than result: 1 or 0
rax = rdi u< rsi   // unsigned less-than result: 1 or 0
jmp .done if rcx == 0
jmp .more if r8 u>= 10
jmp .neg if r9 i< 0
```

Invalid integer comparisons:

```ss
rax = rdi < rsi    // invalid: integer ordering needs signedness
jmp .done if rcx >= 10 // invalid: use i>= or u>=
```

Valid floating-point comparisons:

```ss
mem left:f64 = 1.5
mem right:f64 = 2.25

jmp .less if xmm0 < [right]    // valid: width inferred from f64 memory
jmp .less if xmm0 f64< xmm1    // valid: explicit f64 comparison
jmp .same if xmm0 f32== 0.0    // valid: explicit f32 comparison
```

Invalid floating-point comparisons:

```ss
jmp .less if xmm0 < xmm1 // invalid: XMM registers do not imply f32 or f64
jmp .less if xmm0 < 1.0  // invalid: use f32< or f64<
```

## Condition Results And Conditional Assignment

Assigning a condition stores `1` when the condition is true and `0` when it is false:

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

## Scratch Registers

Some features can use the following registers as scratch storage. Unless an operation explicitly documents preservation, do not rely on these registers retaining their values across the operation.

### x86-64 `r8`, `r9`, `r10`, `r11`

- Arithmetic expression lowering may use these registers as temporaries, preferring `r10` and `r11` when available.
- Integer `min` and `max` assigned to memory may use one of these registers for the result before storing it.
- Integer `sqrt` assigned to memory may use these registers for the input, accumulator, bit mask, and intermediate value.
- Low-result signed or unsigned division and modulo may use `r10` or `r11` to materialize a divisor.
- Widened multiply and divide may use `r10` or `r11` for an immediate or otherwise conflicting right operand.
- Power-of uses `r10` for the mutable base and `r11` for the mutable exponent.

### x86-64 `rax`, `rdx`

- Low-result signed or unsigned division and modulo use the hardware `rax`/`rdx` dividend and result registers.
- Widened signed or unsigned multiply and divide use the hardware `rdx:rax` result pair.
- `linux.syscall` uses `rax` for the syscall number and return value.

### x86-64 `xmm15`

- Floating-point `sqrt`, `min`, `max`, `round`, `floor`, `ceil`, and `trunc` assigned to memory use `xmm15` as the result temporary.
- Do not rely on `xmm15` being preserved across floating-point intrinsic assignments to memory.

### AArch64 Scratch Ranges

- General AArch64 code generation uses `x16` as its primary temporary, including stack-string initialization, memory moves, address materialization, and integer-to-float conversions.
- Integer expression lowering uses `x16` and expands to `x17` for a second operand or intermediate value.
- Division and modulo use `x16` and `x17`, and modulo uses `x18` for the quotient while calculating the remainder.
- Power expressions use `x16` for the mutable base, `x17` for the mutable exponent, and `x18` for the accumulator.
- Numeric runtime formatting uses `x16` through `x21`: `x16` is the value, `x17` is the output pointer, `x18` is the radix, `x19` is the quotient, `x20` is the output digit, and `x21` is the buffer end.
- Floating-point operations and intrinsics use `v16` and `v17` (or their `s`/`d` scalar views) for intermediate operands; `x16` is also used to materialize floating-point literals.
- Linux runtime syscalls use `x0` through `x5` for arguments and `x8` for the syscall number; these registers may be clobbered by runtime operations.
- Do not rely on these AArch64 scratch registers being preserved across arithmetic, floating-point operations, formatted output, memory runtime operations, or Linux syscalls.

### Registers Temporarily Used By `linux.print`

#### x86-64

- Runtime printing temporarily uses `rax`, `rbx`, `rcx`, `rdi`, `rsi`, `rdx`, and `r11` while preparing syscalls and formatting values.
- `linux.print` saves and restores these general-purpose registers for each print part, so they are preserved after the operation.

#### AArch64

- Runtime printing uses `x16` through `x21` for formatting state and output-buffer management.
- The Linux `write` syscall uses `x0` through `x2` for its arguments and `x8` for the syscall number.
- These registers are not currently preserved across AArch64 `linux.print` operations.

### x86-64 Raw `syscall`

- The raw `syscall` instruction clobbers `rcx` and `r11` according to the x86-64 instruction contract.
- The syscall number, arguments, and return value use the Linux syscall register convention; Subsea does not automatically preserve those registers for a raw `syscall`.

## Printing

`linux.print "..."` prints literal text directly. `linux.print rax` prints a runtime integer operand as signed decimal text. Printing does not add a newline automatically:

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
- `{}` infers formatting for compile-time bindings, stack variables, and memory operands with known widths. Raw registers do not carry signedness, so use an explicit placeholder for registers.
- Runtime integer operands use typed placeholders: `{i8}`, `{i16}`, `{i32}`, `{i64}`, `{u8}`, `{u16}`, `{u32}`, `{u64}`, `{x}`, `{b}`, and `{ptr}`.
- Signed placeholders sign-extend their operand before printing; unsigned placeholders zero-extend their operand before printing. The placeholder width must match the operand width, except integer immediates.
- `{x}` and `{ptr}` print lowercase hexadecimal with a `0x` prefix. `{b}` prints binary with a `0b` prefix.

Runtime integer formatting accepts integer immediates, integer `const` values, integer registers, stack variables, and memory operands with matching widths:

```ss
rax = -42
rbx = 42
stack signed:i64 = -7
stack unsigned:u64 = 7

linux.print "inferred signed = {}\n", signed
linux.print "inferred unsigned = {}\n", unsigned
linux.print "signed = {i64}\n", rax
linux.print "unsigned = {u64}\n", rax
linux.print "hex = {x}\n", rbx
linux.print "binary = {b}\n", rbx
linux.print "pointer = {ptr}\n", rbx

al = -1
bl = -1
linux.print "signed byte = {i8}\n", al
linux.print "unsigned byte = {u8}\n", bl
```

For registers, use an explicit format because the same 64 bits can be interpreted multiple ways:

```ss
linux.print "{}\n", rax    // invalid - cannot infer register signedness
linux.print "{i64}\n", rax // signed decimal
linux.print "{u64}\n", rax // unsigned decimal
```

Supported string escapes:

```text
\n
\t
\"
\\
```

Print clobbers:

On x86-64, `linux.print` lowers to the Linux `write` syscall. Runtime integer formatting uses scratch registers internally, but preserves general-purpose registers across each print part. AArch64 print clobbers are documented above.

```text
// x86-64 linux.print preserves general-purpose registers
rax rbx rcx rdx rdi rsi rbp rsp r8 r9 r10 r11 r12 r13 r14 r15
```

Floating-point runtime formatting is not supported yet.

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

## Linux Virtual Memory

`linux.reserve(size)` asks Linux for a readable/writable anonymous virtual memory range. It must be used on the right side of an assignment and leaves the Linux result in the destination. On success, the result is the starting address. On failure, the result is a negative errno value.

`linux.release(ptr, size)` returns a previously reserved virtual memory range to Linux. It leaves the Linux result in `rax`: `0` on success, or a negative errno value on failure.

```ss
main: {
  rax = linux.reserve(4096)
  jmp .error if rax i< 0

  [rax] = "Hi\n"

  stack message:str = slice(rax, 3)
  linux.print message

  linux.release(rax, 4096)
  jmp .error if rax i< 0

  linux.exit 0

.error:
  linux.exit 1
}
```

This is OS virtual memory, not a heap allocator. You must keep track of the pointer and size you reserve so the same range can be released later.

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

### Function Contracts

Functions may optionally document their physical register interface. This does not introduce argument-passing or call syntax: `call` still transfers control to the named function, and the registers remain visible at the call site.

```ss
add: (rdi:u64, rsi:u64) -> rax:u64 [rcx, r8] {
  rax = rdi + rsi
  ret
}
```

The parenthesized registers are inputs, the arrow registers are outputs, and
the optional bracketed registers are clobbers. Multiple outputs use a tuple:

```ss
divide: (rax:u64, rdi:u64) -> (rax:u64, rdx:u64) [rcx] {
  rax = rax
  rdx = rdx
  ret
}
```

Contracts are checked conservatively. A contracted function cannot contain opaque calls, inline assembly, syscalls, or runtime helpers, and source-visible registers must be declared by the contract. Uncontracted functions remain valid and retain the normal Subsea behavior.


Functions use a mixed caller/callee preservation convention.
- A callee may freely modify caller-preserved registers `rax`, `rcx`, `rdx`, `rdi`, `rsi`, and `r8`-`r11` without restoring their values before returning. So, callers must save those registers themselves if they need their values after `call`.
- Registers `rbx`, `rbp`, and `r12`-`r15` are callee-preserved, so a callee that changes them must restore their original values before returning. 

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

Reusable functions and static declarations can be imported explicitly from another `.ss` file. Import paths are relative to the file that contains the import. Imported symbols must be marked with `export`; private helpers and storage remain usable inside the imported file but cannot be imported directly.

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
  asm.x86 "out 0xe9, al"

  rsi = rsi + 1
  rdx = rdx - 1
  jmp .loop

.done:
  ret
}
```

Imports are intentionally narrow: only explicitly listed exported symbols can be imported. Prefix a declaration with `export`, or use a standalone `export <name>` declaration after it:

```ss
// lib/tables.ss
export mem lookup:u8 = [10, 20, 30]

export data metadata section ".metadata" {
  u64 1
}
```

```ss
import lookup, metadata from "lib/tables.ss"

main: {
  al = [lookup]
  rax = &metadata
  linux.exit 0
}
```

Unrequested exports remain private to the imported module. A data block's `export` option still controls its ELF visibility; module exports are controlled separately with `export <name>`. Compile-time `const` bindings are function-local and are not importable.

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

Top-level `mem` declarations allocate static writable memory for the lifetime of the program, similar to Assembly `.data` or `.bss` storage, not heap allocation. Bracketed memory operands load from or store through memory, while `&` computes an address without reading memory.

Use `align` to control the starting address independently of the element width:

```ss
mem packet_buffer:u8(2048) align 64
mem page:u8(4096) align 4096
```

Alignment must be a non-zero power of two and may use a compile-time layout constant. It changes the declaration's starting address, not the number of bytes or the spacing between array elements.

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
mem callback:ptr = addr main  // store's `main`'s address

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
rax = values[8]   // u64 load

bytes[rax] = 72   // u8 store
al = bytes[rax]   // u8 load
```

String literals can be assigned to memory operands to copy their bytes into writable memory. String byte assignment does not add an implicit NUL terminator, and empty string assignments are rejected:

```ss
mem buf:u8(16)

main: {
  const hi = "Hi\n"
  [buf] = hi
  buf[3] = "Bye\n"

  linux.exit 0
}
```

String byte assignment is memory-only. Use a memory destination without an explicit width because the string literal determines the number of bytes written:

```ss
[rax] = "Hi\n"     // valid
buf[0] = "Hi\n"   // valid
[rax] = hi         // valid when hi is a string binding
rax = "Hi\n"      // invalid: destination is not memory
[rax]:u8 = "Hi\n" // invalid: string writes multiple bytes
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

## Compile-Time Layouts

`layout` defines named byte offsets, sizes, and alignments. It does not create a runtime aggregate value and does not add implicit `point.x` memory access.

```ss
layout Point align 8 {
  x:i64
  y:i64
}

mem point:u8(Point.size) align Point.align

[point + Point.x]:i64 = 10
[point + Point.y]:i64 = 20
```

For a byte buffer whose size and alignment are entirely supplied by a layout, the shorthand `mem point:u8(Point)` is equivalent to the declaration above. An explicit larger alignment may still be supplied; a smaller one is rejected.

Fields use natural alignment and padding. The synthesized constants
`Point.x`, `Point.y`, `Point.size`, and `Point.align` are folded into ordinary integer offsets during parsing. Layout declarations currently use scalar widths, are module-local, and must appear before use. `layout` alignment may increase natural alignment but cannot reduce a field's required alignment.

## Stack Variables

Use `stack` to declare function-local storage in the current function's stack frame:

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

Stack variables live from function entry to function exit, not from the declaration line. A `stack` declaration inside a loop does not allocate once per iteration.

If a function declares stack variables, Subsea reserves `rbp` for the stack frame in that function. Do not read or write `rbp`, `ebp`, `bp`, or `bpl` manually in a function that uses `stack`.

The stack `str` type is a descriptor stored as an address and a byte length on the stack. The descriptor is stored in the current function's stack frame, but its backing bytes are not necessarily on the stack. Stack string descriptors are currently initialized only at declaration; whether the bytes can be modified depends on their backing storage.

A literal initializer points at compiler-emitted read-only bytes:

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

A stack string can also be a view over writable `mem` storage. The descriptor remains in the stack frame, while changing the backing bytes changes what the string prints:

```ss
mem text:u8(16)

main: {
  [text] = "Hello"
  stack message:str = slice(&text, 5)

  linux.print message  // Hello
  linux.print "\n"

  text[0] = 74       // ASCII 'J'
  linux.print message  // Jello
  linux.print "\n"

  linux.exit 0
}
```

Use a stack byte buffer when the writable backing storage should be local to the current function - unlike global `mem`. The buffer has a fixed capacity, is zero-initialized, and can be used as the backing storage for a stack `str` descriptor:

```ss
main: {
  stack buffer:u8(16)
  [buffer] = "Hello"

  stack message:str = slice(&buffer, 5)
  linux.print message
  linux.print "\n"

  buffer[0]:u8 = 74       // ASCII 'J'
  linux.print message
  linux.print "\n"

  linux.exit 0
}
```

The buffer is function-local and fixed-size. Indexed access is low-level and does not perform runtime bounds checks; keep accesses within the declared capacity.

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

Use `::i8`, `::i16`, `::i32`, `::i64`, or the corresponding unsigned widths to cast floating-point operands into integer registers. Float-to-int casts truncate toward zero. Register sources must carry an explicit floating-point width (`s`/`d` on AArch64); XMM sources default to `f64` on x86-64.

```ss
mem ratio:f64 = 1.5
rax = [ratio]::i64
ecx = [ratio]::i32
```

Direct floating-register-to-integer casts are supported on both backends:

```ss
rax = xmm0::i64       // x86-64, defaults to f64
x0 = s0::i32          // AArch64, f32 source
```

Runtime float-to-integer casts truncate toward zero when the value is within
the destination range. NaN, negative unsigned values, and out-of-range values
error.

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

Runtime float printing is not yet supported.

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
- `min`, `max`, `sqrt`, `round`, `floor`, `ceil`, and `trunc` support scalar floating widths `f32` and `f64`; floating-point destinations may be XMM registers or matching floating-point memory operands.
- Integer `sqrt` supports signed and unsigned widths from 8 to 64 bits, and returns the floor square root. Signed inputs must be non-negative; negative immediate values are rejected at compile time, while negative runtime values trap.
- Integer result rounding is not implemented yet; `round(...):i64` and other integer widths are rejected.
- Floating-point rounding emits SSE4.1 `roundss` or `roundsd`: `round` uses nearest, `floor` rounds down, `ceil` rounds up, and `trunc` rounds toward zero.
- Floating-point intrinsic operands can be XMM registers, floating-point memory operands, `f32`/`f64` const bindings, stack float variables, or float literals matching the intrinsic width.

## Freestanding And Raw Assembly

`asm.x86 "..."` emits one raw x86-64 assembly instruction, while `asm.aarch64 "..."` emits one raw AArch64 instruction. These forms are useful for explicit architecture interop in freestanding code and must match the selected target.

Freestanding halt loop:

```ss
main: {
.hang:
  asm.x86 "hlt"
  jmp .hang
}
```

Port I/O can be written as raw x86 assembly. The examples below use byte-sized `in`/`out`: `al` is the data register, and the port must be an immediate `0..255` or `dx`.

```ss
main: {
  al = 72
  asm.x86 "out 0x80, al"

  asm.x86 "in al, dx"

.hang:
  asm.x86 "hlt"
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
  asm.x86 "hlt"
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

Subsea uses real target register names.

### x86-64 Registers

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

### AArch64 Registers

Supports the general-purpose AArch64 registers and their 32-bit aliases:

```text
x0 x1 x2 x3 x4 x5 x6 x7 x8 x9 x10 x11 x12 x13 x14 x15
x16 x17 x18 x19 x20 x21 x22 x23 x24 x25 x26 x27 x28 x29 x30
w0 w1 w2 w3 w4 w5 w6 w7 w8 w9 w10 w11 w12 w13 w14 w15
w16 w17 w18 w19 w20 w21 w22 w23 w24 w25 w26 w27 w28 w29 w30
sp wsp
```

It also supports the AArch64 SIMD/scalar register names:

```text
v0-v31   q0-q31   d0-d31   s0-s31   h0-h31   b0-b31
```

The `v` and `q` forms name 128-bit SIMD registers; `d`, `s`, `h`, and `b` select scalar views of those registers.

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
subsea run -t aarch --runner qemu-aarch64 main.ss // Cross-run with an explicit runner
subsea build main.ss      // Compile, assemble, and link an executable
subsea emit-asm main.ss   // Compile to target assembly and print it
subsea emit-asm --annotate main.ss // Include source locations and statements
```

`--annotate` adds source comments to emitted assembly. Comments appear before the assembly they describe, imported instructions retain their source file locations, and compiler-generated regions are marked explicitly. This makes the output useful when learning or auditing the generated machine code.

> `run` exits with the compiled program's exit code.

`run` accepts Linux targets (`x86` and `aarch`). It executes the output directly by default; use `--runner <program>` for an explicit cross-target runner such as `qemu-aarch64`. Arguments after `--` are passed to the compiled program unchanged:

```sh
subsea run -t aarch --runner qemu-aarch64 main.ss -- argument-one --flag
```

### targets

The default target is `x86`, which means x86-64 Linux userland. This target supports Linux helpers such as `linux.print`, `linux.read(stdin, ...)`, and `linux.exit`.

Subsea also supports AArch64 Linux with the `aarch` target. The language features are target-independent where possible, including integer and floating-point operations, control flow, stack strings, memory operations, formatted output, and Linux runtime helpers. AArch64 output uses the AArch64 Linux toolchain and can be selected with `--target aarch` or `-t aarch`.

```sh
subsea emit-asm --target aarch main.ss
subsea build --target aarch main.ss
```

Use `x86-free` for freestanding x86-64 assembly. This target still emits x86-64 instructions, but it rejects Linux-only helpers because there may be no Linux process, stdout, stdin, or process exit:

```sh
subsea emit-asm --target x86-free kernel.ss
subsea emit-asm -t x86-free kernel.ss
subsea build --target x86-free -o kernel.o kernel.ss
subsea build --target x86-free --linker-script kernel.ld -o kernel.elf kernel.ss
subsea build --target x86-free -T kernel.ld --format binary -o kernel.bin kernel.ss
```

Use `aarch-free` for freestanding AArch64 assembly. It has the same freestanding build flow and Linux-runtime restrictions, but emits AArch64 instructions and uses the AArch64 target toolchain:

```sh
subsea emit-asm --target aarch-free kernel.ss
subsea build --target aarch-free -o kernel.o kernel.ss
subsea build --target aarch-free -T kernel.ld -o kernel.elf kernel.ss
subsea build --target aarch-free -T kernel.ld --format binary -o kernel.bin kernel.ss
```

By default, Subsea emits source-level `main` as the linker-visible symbol `_start`. Freestanding mode can override that symbol with `--entry`:

```sh
subsea emit-asm -t aarch-free --entry kernel_entry kernel.ss
```

For freestanding targets, `build` writes an object file instead of linking an executable by default. This keeps freestanding output composable with external boot code, linker scripts, and custom build systems. If `-o` is omitted, the object file is written to `target/subsea/main.o`.

Pass `--linker-script` or `-T` to link the freestanding object with `ld -T` and write a freestanding executable instead:

```sh
subsea build -t x86-free -T kernel.ld -o kernel.elf kernel.ss
```

Freestanding linker-script builds pass the target's linker emulation (for example, `elf_x86_64` or `aarch64elf`) to the linker so output does not depend on the host linker's default emulation. Use `--linker` to select a different linker program, such as `ld.lld` or a cross-target linker.

Raw binary output is available for linked freestanding builds with `--format binary`. It links an ELF first, then runs `objcopy -O binary`:

```sh
subsea build -t x86-free -T kernel.ld --format binary -o kernel.bin kernel.ss
```

Freestanding support is early. Raw binaries are not bootable by themselves; they still need a boot sector, firmware header, bootloader protocol metadata, or an external bootloader before a QEMU smoke test is meaningful.

See `examples/freestanding` for object/ELF/raw-binary examples, and `examples/limine` for a minimal Limine-oriented kernel ELF with request metadata declared in `.ss` data blocks. The Limine example documents a manual QEMU debug-port smoke test; it is not run by `cargo test` because it depends on external Limine binaries, ISO creation tools, and QEMU.

### Limine/QEMU smoke test

From `examples/limine`, build the kernel into the ISO staging tree:

```sh
subsea build -t x86-free -T kernel.ld -o iso_root/boot/kernel.elf kernel.ss
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

If `kernel.ss` writes bytes with `asm.x86 "out 0xe9, al"`, they appear in the terminal where QEMU is running. The QEMU display window may stay blank because this example does not write to the framebuffer.

### build flags

Writes intermediate assembly and object files to a unique per-build directory under `target/subsea/build-*`. The default executable is written to `target/subsea/main`

`-o`: output is written to the requested path. For Linux targets, this is a linked executable. For freestanding targets, this is an object file unless `--linker-script`/`-T` is provided.

```sh
subsea build -o my_util main.ss
subsea build -t x86-free -o kernel.o kernel.ss
```

`--linker-script` or `-T`: link freestanding output with the requested linker script. This writes a freestanding executable instead of an object file.

```sh
subsea build -t x86-free --linker-script kernel.ld -o kernel.elf kernel.ss
subsea build -t x86-free -T kernel.ld -o kernel.elf kernel.ss
```

`--format`: select the linked freestanding output format. Supported values are `elf` and `binary`. `binary` requires `--linker-script`/`-T`.

```sh
subsea build -t x86-free -T kernel.ld --format elf -o kernel.elf kernel.ss
subsea build -t x86-free -T kernel.ld --format binary -o kernel.bin kernel.ss
```

`--linker`: select the linker program for freestanding linker-script builds. The default comes from the selected target.

```sh
subsea build -t x86-free --linker ld.lld -T kernel.ld -o kernel.elf kernel.ss
```

`--link-input`: add an extra object file to a freestanding linker-script build. This flag can be repeated.

```sh
subsea build -t x86-free -T kernel.ld --link-input boot.o --link-input tables.o -o kernel.elf kernel.ss
```

`--target` or `-t`: select a target. Supported targets are `x86`, `x86-free`, `aarch`, and `aarch-free`.

```sh
subsea build -t x86-free kernel.ss
```

`--entry`: select the linker-visible entry symbol for a freestanding target. The source program still uses `main`; codegen emits that entry block with the requested symbol.

```sh
subsea build -t x86-free --entry kernel_entry kernel.ss
```

`--timings`: show build times for various phases of the process.

```sh
subsea build --timings main.ss
```
