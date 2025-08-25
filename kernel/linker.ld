ENTRY(kmain)

SECTIONS
{
    . = 0xFFFFFFFFC0000000;

    .text : ALIGN(16) {
        *(.text.kmain)
        *(.text .text.*)
    }
    
    .rodata : ALIGN(16) {
        *(.rodata .rodata.*)
    }

    .data : ALIGN(16) {
        *(.data .data.*)
    }

    .bss : ALIGN(16) {
        *(.bss .bss.*)
        *(COMMON)
    }
}