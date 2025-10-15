ENTRY(_start)

SECTIONS
{
    . = 0x8000;

    .text : ALIGN(4) {
        *(.text._start)
        *(.text .text.*)
    }
    
    .rodata : ALIGN(4) {
        *(.rodata .rodata.*)
    }

    .bss : ALIGN(4) {
        *(.bss .bss.*)
        *(COMMON)
    }

    .data : ALIGN(4) {
        *(.data .data.*)
    }
}