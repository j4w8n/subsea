.intel_syntax noprefix
.global _start

.equ HEADER_TAG, 0
.equ HEADER_LENGTH, 4
.equ HEADER_LOCATION, 8
.equ HEADER_SIZE, 16
.equ HEADER_ALIGN, 8

.section .bss
.balign HEADER_ALIGN
header:
  .zero HEADER_SIZE

.section .text
_start:
  mov byte ptr [rip + header + HEADER_TAG], 1
  mov dword ptr [rip + header + HEADER_LENGTH], 1024
  mov eax, dword ptr [rip + header + HEADER_LENGTH]
  cmp eax, 1024
  je valid

  mov eax, 60
  mov edi, 1
  syscall

valid:
  mov eax, 60
  xor edi, edi
  syscall
