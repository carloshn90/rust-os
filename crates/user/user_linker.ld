ENTRY(_start)

SECTIONS
{
    /* 1. Tell the compiler to expect runtime addresses at 0x00100000 */
    . = 0x00100000;

    /* Aligning to 4K ensures this becomes its own distinct ELF PT_LOAD segment */
    .text : ALIGN(4K) {
        *(.text._start)
        *(.text .text.*)
    }

    .rodata : ALIGN(4K) {
        *(.rodata .rodata.*)
    }

    .data : ALIGN(4K) {
        *(.data .data.*)
    }

    .bss : ALIGN(4K) {
        *(.bss .bss.*)
    }

    /DISCARD/ : {
        *(.eh_frame)
        *(.note .note.*)
        *(.metadata)
    }
}
