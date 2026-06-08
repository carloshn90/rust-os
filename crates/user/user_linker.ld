ENTRY(_start)

SECTIONS
{
    /* 1. Tell the compiler to expect runtime addresses at 0x00100000 */
    . = 0x00100000;

    /* 2. Force the text section to be output at file offset 0 */
    .text : AT(0) {
        *(.text._start)
        *(.text .text.*)
    }

    .rodata : {
        *(.rodata .rodata.*)
    }

    .data : {
        *(.data .data.*)
    }

    .bss : {
        *(.bss .bss.*)
    }

    /DISCARD/ : {
        *(.eh_frame)
        *(.note .note.*)
        *(.metadata)
    }
}
