SECTIONS
{
    . = 0x10000000000;

    .text : ALIGN(1024) {
        *(.text .text.*)
    }
    
    .rodata : ALIGN(1024) {
        *(.rodata .rodata.*)
    }

    .data : ALIGN(1024) {
        *(.data .data.*)
    }

    .bss : ALIGN(1024) {
        *(.bss .bss.*)
    }
}