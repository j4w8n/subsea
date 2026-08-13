data limine_requests_start section ".limine_requests_start" align 8 keep {
  u64 0xf6b8f4b39de7d1ae
  u64 0xfab91a6940fcb9cf
}

data limine_requests_end section ".limine_requests_end" align 8 keep {
  u64 0xadc0e0531bb10d03
  u64 0x9572709f31764c62
}

main: {
.hang:
  hlt
  jmp .hang
}
