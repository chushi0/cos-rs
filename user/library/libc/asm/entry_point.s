extern main
extern __cos_libc_entry

section .text
global _start

[bits 64]

_start:
    xor rbp, rbp
    and rsp, 0xfffffffffffffff0
    add rsp, 0x8
    mov rdi, main
    jmp __cos_libc_entry
