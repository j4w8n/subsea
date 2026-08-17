.intel_syntax noprefix
.global _start

.section .data
handlers:
  .quad add_ten, double_value, subtract_three

.section .bss
decimal_buf:
  .skip 32

.section .rodata
prefix:
  .ascii "result="
prefix_len = . - prefix
newline:
  .ascii "\n"

.section .text
_start:
  mov rdi, 7          # input value
  mov rax, 1          # handler index
  lea r8, [rip + handlers]
  call qword ptr [r8 + rax * 8]

  mov r12, rax
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

add_ten:
  lea rax, [rdi + 10]
  ret

double_value:
  lea rax, [rdi + rdi]
  ret

subtract_three:
  lea rax, [rdi - 3]
  ret
