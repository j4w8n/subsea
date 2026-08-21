.intel_syntax noprefix
.global _start

.section .rodata
result:
  .ascii "Result is "
result_len = . - result
newline:
  .ascii "\n"

.section .bss
decimal:
  .skip 32

.section .text
_start:
  mov rdi, 42
  mov rsi, 17
  call add

  mov r12, rcx
  mov rax, 1
  mov rdi, 1
  lea rsi, [rip + result]
  mov rdx, result_len
  syscall

  mov rax, r12
  lea rdi, [rip + decimal + 32]
  mov rbx, 10

format_loop:
  xor rdx, rdx
  div rbx
  sub rdi, 1
  add dl, '0'
  mov byte ptr [rdi], dl
  test rax, rax
  jne format_loop

  lea rdx, [rip + decimal + 32]
  sub rdx, rdi
  mov rax, 1
  mov rsi, rdi
  mov rdi, 1
  syscall

  mov rax, 1
  mov rdi, 1
  lea rsi, [rip + newline]
  mov rdx, 1
  syscall

  mov rax, 60
  xor rdi, rdi
  syscall

add:
  mov rcx, rdi
  add rcx, rsi
  ret
