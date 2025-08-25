[org 0x2000]

    bits 32
    ; 远跳进入长模式
    jmp 0x18:.long_mode

.long_mode:
    bits 64
    ; 设置各段寄存器
    mov ax, 0x20
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    mov ss, ax
    ; 设置内核栈
    mov rsp, 0xFFFF_FFFF_FFDF_FFF8
    ; 调用内核kmain
    jmp 0xFFFF_FFFF_C000_0000
