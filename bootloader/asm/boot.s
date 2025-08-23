; BIOS自检完成后，会将引导记录的代码加载到0x7c00
[org 0x7c00]
    bits 16

    ; 通常情况下，BIOS会加载到0x0000:0x7c00（段0，地址0x7c00）
    ; 但某些BIOS会加载到0x07c0:0x0000（段0x07c0，地址0)
    ; 对应的物理地址实际上是一样的，我们强制固定一下
    jmp 0x0000:.start

.start:

    ; 初始化各段寄存器
    ; 段寄存器无法使用立即数修改，我们借助另一个寄存器进行修改
    mov ax, 0
    mov ds, ax ; 数据段寄存器
    mov ss, ax ; 栈段寄存器
    mov es, ax ; 附加段寄存器

    ; 初始化栈寄存器，BIOS并不一定给我们准备好栈
    ; 我们的栈从0x7c00向下增长
    mov sp, 0x7c00 ; 栈指针寄存器
    mov bp, sp     ; 栈帧指针寄存器

    ; BIOS给我们传了一些参数，我们记录下来
    mov [.startup_disk], dl; 启动磁盘

    ; 目前显示器输出为VGA文本模式，屏幕上可能还会残留BIOS的信息
    ; 我们通过BIOS功能清除屏幕上的所有内容
    mov ax, 3
    int 0x10

    ; 在屏幕上输出一些正在启动的信息
    mov si, .boot_msg
    call .print

    ; 检测A20地址线是否开启，如果未开启，尝试开启
    call .is_a20_enable
    test al, al
    jnz .finish_a20 ; A20已经开启

    ; 尝试快速开启
    in al, 0x92
    or al, 2
    out 0x92, al

    call .is_a20_enable ; 重新测试
    test al, al
    jnz .finish_a20 ; A20已经开启

    mov si, .enable_a20_err_msg
    call .print
    jmp .hlt_loop

.finish_a20:
    
    ; 从磁盘中读取磁盘扇区到内存0x8000
    ; 我们提前将Loader程序加载进来，因为一旦进入32位保护模式，我们将无法通过BIOS功能加载磁盘
    mov ax, 0
    mov es, ax              ; 设置段寄存器
    mov ah, 0x02            ; 读磁盘功能
    mov al, [.loader_size]  ; 扇区数量
    test al, al
    jz .load_error          ; 如果扇区数量为0，直接报错
    mov ch, 0               ; 柱面号
    mov cl, 2               ; 起始扇区号（从1开始记数，boot算第一个扇区）
    mov dh, 0               ; 磁头号，双面软盘的话，0/1表示两面
    mov dl, [.startup_disk] ; 磁盘号，dl
    mov bx, 0x8000          ; 目标地址，es:bx = 0x0000:0x8000
    int 0x13                ; 调用BIOS磁盘中断
    jc .load_error          ; 如果CF=1，则读取失败

    ; 进行内存检测
    ; 一旦进入32位保护模式，我们就失去了BIOS功能，我们需要尽早完成内存检测
    mov di, 0x1000       ; 结果存储在0x1000处
    xor ebx, ebx         ; ebx=0（首次调用）
    mov edx, 0x534D4150  ; 魔数，SMAP
    mov cx, 20           ; 每次读取24字节的内存描述符
    mov si, 0            ; 记数
.mem_detect_next:
    mov eax, 0xe820      ; 功能号
    int 0x15
    jc .mem_detect_err   ; 检测失败
    add di, cx           ; 移动到下一个描述符位置
    inc si               ; 记数
    test ebx, ebx
    jnz .mem_detect_next ; ebx!=0则继续
    mov [.mem_struct_size], si
    
    ; 关中断，我们还没有准备好32位保护模式下的中断处理，开启中断会triple fault
    cli

    ; 设置GDT（全局描述符表）
    ; 加载GDT基址和长度到GDTR
    lgdt [.gdt_descriptor]

    ; 设置CR0的PE位，准备启用32位保护模式
    mov eax, cr0
    or eax, 0x1
    mov cr0, eax

    ; 进入32位保护模式
    ; 在设置GDT、设置CR0的PE位并使用far jmp后，才会真正进入32位保护模式
    jmp 0x08:.protected_mode_entry

; 空转，等待中断，使用hlt减少CPU功耗
.hlt_loop:
    hlt
    jmp .hlt_loop

; 子功能，向屏幕上输出文本
; 调用前，将si指向字符串，然后使用call调用
.print:
    mov ax, 0xb800     ; VGA文本模式地址
    mov es, ax         ; 用段寄存器指向地址
    mov di, 0          ; di为偏移量，es:di为循环中写入的内容
    mov dh, 0x07       ; 颜色属性，黑底白字
.boot_msg_loop:
    mov dl, [si]       ; 读取当前字符
    test dl, dl        ; 检测是否为终止符（0）
    jz .boot_msg_end   ; 为0则跳转
    mov es:[di], dx    ; 写入内存
    inc si             ; si自增
    add di, 2          ; di加2，因为一个字符占2字节空间
    jmp .boot_msg_loop ; 循环
.boot_msg_end:
    ret

; 子功能，判断A20地址线是否开启
; 此功能通过判断0000:7dfe与ffff:7e0e是否为同一个内存地址来判断
.is_a20_enable:
    ; 首先判断ffff:7e0e是否为0xAA55
    mov ax, 0xffff
    mov ds, ax
    mov bx, 0x7e0e
    mov cx, ds:[bx] ; 将ffff:7e0e读取到cx
    cmp cx, 0xaa55 ; 判断cx与0xaa55是否相等
    jnz .a20_enable ; 不相等，说明a20地址线开启

    ; ffff:7e0e与0000:7dfe 相等
    ; 我们先修改0000:7dfe，然后查看ffff:7e0e是否同时变化
    mov ax, 0
    mov ds, ax
    mov bx, 0x7dfe
    mov word ds:[bx], 0xa5a5 ; 将 0000:7dfe修改为0xa5a5

    mov ax, 0xffff
    mov ds, ax
    mov bx, 0x7e0e
    mov cx, ds:[bx] ; 将ffff:7e0e读取到cx

    mov ax, 0
    mov ds, ax
    mov bx, 0x7dfe
    mov word ds:[bx], 0xaa55 ; 将 0000:7dfe修改为0xaa55

    cmp cx, 0xa5a5 ; 判断cx与0xa5a5是否相等
    jnz .a20_enable ; 不相等，说明a20地址线开启

    mov al, 0
    ret
.a20_enable:
    mov ax, 0
    mov ds, ax
    mov al, 1
    ret


; loader程序加载失败
.load_error:
    mov si, .load_error_msg
    call .print
    jmp .hlt_loop

; 内存检测失败
.mem_detect_err:
    mov si, .mem_detect_err_msg
    call .print
    jmp .hlt_loop

; 进入32位保护模式
.protected_mode_entry:
    bits 32

    ; 初始化32位栈
    mov ax, 0x10
    mov ss, ax
    mov esp, 0x7c00

    ; 初始化其他段寄存器
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax

    ; 调用loader程序
    mov dword [.mem_struct_addr], 0x1000
    push dword .project_info
    push dword .bios_info
    call dword 0x8000


; GDT定义
; GDT 是一个连续的内存数组，每个元素是一个8 字节的段描述符，最多可包含 8192 个描述符
; 
; 1. 基地址（32位）
; 定义段在物理内存中的起始地址（32 位，范围 0~4GB），分 3 部分存储：
; - 低16位：描述符中的2-3字节
; - 中8位：描述符中的4字节
; - 高8位：描述符中的7字节
;
; 2. 限长（20位）
; 定义段的最大长度（字节数），分两部分存储：
; 低16位：描述符中的0-1字节
; 高4位：描述符中的6字节
; 实际长度计算依赖粒度G标志：G=0则单位为字节，最大长度=1M；G=1则单位为4kb，最大长度为4G
;
; 3. 访问权限字节
; 格式：P DPL S Type（从高到低），每bit含义：
; 7位 P（存在），若P=1则表示该段在内存中存在；P=0表示不存在，访问会触发异常
; 6-5位 DPL（特权级），0~3，用于内存保护（0为最高特权级，3为最低）
; 4位 S（描述符类型），S=1代码/数据段描述符；S=0系统段描述符（TSS、LDT）
; 3-0位 Type（段类型）（当S=1时）
;  - 代码段（可执行）：1 C R A
;    C=1表示一致代码段，低特权级程序可执行；C=0表示非一致代码段，仅同特权级可执行
;    R=1表示可读；R=0表示不可读
;    A=1表示段已被访问（CPU自动设置）
;  - 数据段（不可执行）：0 E W A
;    E=1表示段向上扩展（从基地址向高地址增长）；E=0表示段向下扩展（从限长向低地址增长）
;    W=1表示数据段可写，W=0表示数据段只读
;    A=1表示段已被访问（CPU自动设置）
;
; 4. 标志字节
; 包含描述符中6字节高4位
; 7位 G（粒度），G=0表示限长单位为字节，G=1表示限长单位为4KB
; 6位 D/B（默认操作数大小），32位模式下，D=1表示操作数/指针为32位，D=0表示16位（兼容实模式）
; 5位 L（长模式），L=1表示64位代码段，仅长模式有效；32位模式下必须为0
; 4位 AVL，由软件自由使用，CPU忽略
;
; 设置GDT后，段寄存器（CS/DS/ES）被称为“选择子”，16位结构为
; 索引（13位）TI（1位）RPL（2位）
; 索引：用于选择GDT中的描述符
; TI（表指示器）：TI=0表示从GDT索引，TI=1表示从LDT（局部描述符表）索引
; RPL（请求特权级）：0~3，需<=段的DPL才能访问
.gdt_start:
    ; 空描述符，必须存在，第0项
    dd 0x0
    dd 0x0

    ; 代码段描述符（第1项，0x8选择子）
    ; 基地址=0x0，限长0xfffff（4G），权限=可执行、特权级0
    dw 0xffff     ; 限长（低16位）
    dw 0x0        ; 基地址（低16位）
    db 0x0        ; 基地址（中8位）
    db 0b10011010 ; 访问权限，P=1（存在），DPL=0，代码段，可执行，可读
    db 0b11001111 ; 粒度，G=1（4kb），L=0（非64位），限长（高4位）
    db 0x0        ; 基地址（高8位）

    ; 数据段描述符
    ; 基地址=0x0，限长=0xfffff（4G），权限=可读/写
    dw 0xffff
    dw 0x0
    db 0x0
    db 0b10010010 ; 访问权限，P=1，DPL=0，数据段，可写
    db 0b11001111
    db 0x0
.gdt_end:

.gdt_descriptor:
    dw .gdt_end - .gdt_start - 1 ; GDT长度（总字节数-1）
    dd .gdt_start                 ; GDT基地址

.boot_msg:
db "COS Booting...", 0
.enable_a20_err_msg:
db "failed to enable a20", 0
.load_error_msg:
db "failed to load loader", 0
.mem_detect_err_msg:
db "failed to detect memory", 0

.bios_info:

; 内存检测地址
.mem_struct_addr:
dw 0, 0
; 内存检测结构体长度
.mem_struct_size:
dw 0
; 存储启用的磁盘号
.startup_disk:
db 0

times 510 - 3 - ($ - $$) db 0

.project_info:

; 我们的构建程序会将loader程序的扇区数量写在这里
.loader_size:
db 0

; 我们的构建程序会将kernel程序的扇区数量写在这里
.kernel_size:
dw 0

db 0x55, 0xaa
