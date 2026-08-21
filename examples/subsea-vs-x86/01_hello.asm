.intel_syntax noprefix
.global _start

.section .rodata
message:
  .ascii "Hello from x86 assembly!\n"
message_len = . - message

.section .text
_start:
  mov rax, 1          # write
  mov rdi, 1          # stdout
  lea rsi, [rip + message]
  mov rdx, message_len
  syscall

  mov rax, 60         # exit
  xor rdi, rdi        # status 0
  syscall
