ENTRY(kmain)

SECTIONS
{
    . = 0xFFFFFFFFC0000000;

    .text : ALIGN(8) {
        *(.text.kmain)
        *(.text .text.*)
    }
    
    .rodata : ALIGN(8) {
        *(.rodata .rodata.*)
    }

    .data : ALIGN(8) {
        *(.data .data.*)
    }

    .bss : ALIGN(8) {
        *(.bss .bss.*)
        *(COMMON)
    }

    .other : ALIGN(8) {
        *(.dynsyn)
        *(.gnu.hash)
        *(COMMON)
    }
}