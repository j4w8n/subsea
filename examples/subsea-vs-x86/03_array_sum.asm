.intel_syntax noprefix
.global _start

.section .data
values:
  .quad 5, 10, 20, 40
value_count = 4

.section .bss
decimal_buf:
  .skip 32

.section .rodata
prefix:
  .ascii "sum="
prefix_len = . - prefix
newline:
  .ascii "\n"

.section .text
_start:
  xor rax, rax        # sum
  xor rcx, rcx        # index
  lea r8, [rip + values]

sum_loop:
  cmp rcx, value_count
  jae print_sum
  add rax, qword ptr [r8 + rcx * 8]
  inc rcx
  jmp sum_loop

print_sum:
  mov r12, rax        # save sum across write syscalls

  mov rax, 1
  mov rdi, 1
  lea rsi, [rip + prefix]
  mov rdx, prefix_len
  syscall

  mov rax, r12
  lea rdi, [rip + decimal_buf + 32]
  mov rbx, 10

convert_loop:
  xor rdx, rdx
  div rbx
  dec rdi
  add dl, '0'
  mov byte ptr [rdi], dl
  test rax, rax
  jne convert_loop

  lea rdx, [rip + decimal_buf + 32]
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
