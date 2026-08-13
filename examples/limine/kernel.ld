ENTRY(_start)

PHDRS
{
  text PT_LOAD FLAGS((1 << 0) | (1 << 2));
  rodata PT_LOAD FLAGS((1 << 2));
  data PT_LOAD FLAGS((1 << 1) | (1 << 2));
}

SECTIONS
{
  . = 0xffffffff80000000;

  .text : ALIGN(4K) {
    *(.text .text.*)
  } :text

  .rodata : ALIGN(4K) {
    *(.rodata .rodata.*)
  } :rodata

  .limine_requests : ALIGN(4K) {
    KEEP(*(.limine_requests_start))
    KEEP(*(.limine_requests))
    KEEP(*(.limine_requests_end))
  } :rodata

  .data : ALIGN(4K) {
    *(.data .data.*)
  } :data

  .bss : ALIGN(4K) {
    *(COMMON)
    *(.bss .bss.*)
  } :data
}
