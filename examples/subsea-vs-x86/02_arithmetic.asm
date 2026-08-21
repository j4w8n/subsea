.intel_syntax noprefix
.global _start

.section .text
_start:
  mov rax, 10
  add rax, 5
  sub rax, 1

  mov rbx, rax
  imul rbx, 3
  add rbx, 2

  xor rcx, rcx
  cmp rbx, 40
  setg cl

  cmp rbx, 40
  jle below_or_equal
  mov rdx, 99
  jmp done

below_or_equal:
  mov rdx, 11

done:
  mov rax, 60
  xor rdi, rdi
  syscall
