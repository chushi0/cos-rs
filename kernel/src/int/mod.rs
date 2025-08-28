use core::{
    arch::asm,
    ops::{Deref, DerefMut},
};

mod soft;

/// 中断描述符表
#[repr(transparent)]
struct IDT([IDTEntry; 256]);

/// 中断描述符表寄存器
#[repr(C, packed)]
struct IDTR<'idt> {
    limit: u16,
    base: &'idt IDT,
}

/// 中断描述符表中的每一项
#[derive(Debug, Clone, Copy)]
#[repr(C)]
struct IDTEntry {
    function_pointer_lower: u16,
    gdt_selector: u16,
    // 从低到高
    // 0-2位 中断栈索引，0表示不要切换栈，其余表示对应的中断栈表的第几项
    // 3-7位 保留
    // 8位 函数被调用时是否禁用中断，0表示禁用，1表示不禁用
    // 9-11位 必须为1
    // 12位 必须为0
    // 13-14位 权限等级
    // 15位 0表示无效
    options: u16,
    function_pointer_middle: u16,
    function_pointer_high: u32,
    reserved: u32,
}

/// 中断处理函数，【必须】通过interrupt_handler宏创建，这个宏处理了x86-interrupt的调用约定，并保存了上下文
type IntFn = extern "C" fn();

/// 中断调用上下文，通过修改对应值可以改变中断返回时对应寄存器的值
#[derive(Debug)]
#[repr(C)]
#[allow(unused)]
struct StackFrame {
    r15: u64,
    r14: u64,
    r13: u64,
    r12: u64,
    r11: u64,
    r10: u64,
    r9: u64,
    r8: u64,
    rbp: u64,
    rdi: u64,
    rsi: u64,
    rdx: u64,
    rcx: u64,
    rbx: u64,
    rax: u64,
    rip: u64,
    cs: u64,
    rflags: u64,
    rsp: u64,
    ss: u64,
}

/// 中断调用上下文，通过修改对应值可以改变中断返回时对应寄存器的值
///
///  与[StackFrame]的区别是包含了error_code
#[derive(Debug)]
#[repr(C)]
#[allow(unused)]
struct StackFrameWithErrorCode {
    r15: u64,
    r14: u64,
    r13: u64,
    r12: u64,
    r11: u64,
    r10: u64,
    r9: u64,
    r8: u64,
    rbp: u64,
    rdi: u64,
    rsi: u64,
    rdx: u64,
    rcx: u64,
    rbx: u64,
    rax: u64,
    error_code: u64,
    rip: u64,
    cs: u64,
    rflags: u64,
    rsp: u64,
    ss: u64,
}

/// 主CPU的中断描述符表
static mut MAIN_CPU_IDT: IDT = IDT::new();

/// 初始化中断
///
/// Safety: 仅在第一次调用时，该函数是安全的。不能并发
pub unsafe fn init() {
    // Safety: 由调用者保证无并发
    unsafe {
        MAIN_CPU_IDT[IDT::INDEX_DIVIDE_ERROR].set_function_pointer(soft::divide_by_zero);
        MAIN_CPU_IDT[IDT::INDEX_DIVIDE_ERROR].enable();
        MAIN_CPU_IDT[IDT::INDEX_BREAKPOINT].set_function_pointer(soft::breakpoint);
        MAIN_CPU_IDT[IDT::INDEX_BREAKPOINT].enable();
        MAIN_CPU_IDT[IDT::INDEX_INVALID_OPCODE].set_function_pointer(soft::invalid_opcode);
        MAIN_CPU_IDT[IDT::INDEX_INVALID_OPCODE].enable();
        MAIN_CPU_IDT[IDT::INDEX_DOUBLE_FAULT].set_function_pointer(soft::double_fault);
        MAIN_CPU_IDT[IDT::INDEX_DOUBLE_FAULT].enable();
        MAIN_CPU_IDT[IDT::INDEX_INVALID_TSS].set_function_pointer(soft::invalid_tss);
        MAIN_CPU_IDT[IDT::INDEX_INVALID_TSS].enable();
        MAIN_CPU_IDT[IDT::INDEX_SEGMENT_NOT_PRESENT]
            .set_function_pointer(soft::segment_not_present);
        MAIN_CPU_IDT[IDT::INDEX_SEGMENT_NOT_PRESENT].enable();
        MAIN_CPU_IDT[IDT::INDEX_STACK_SEGMENT_FAULT]
            .set_function_pointer(soft::stack_segment_fault);
        MAIN_CPU_IDT[IDT::INDEX_STACK_SEGMENT_FAULT].enable();
        MAIN_CPU_IDT[IDT::INDEX_GENERAL_PROTECTION].set_function_pointer(soft::general_protection);
        MAIN_CPU_IDT[IDT::INDEX_GENERAL_PROTECTION].enable();
        MAIN_CPU_IDT[IDT::INDEX_PAGE_FAULT].set_function_pointer(soft::page_fault);
        MAIN_CPU_IDT[IDT::INDEX_PAGE_FAULT].enable();
        MAIN_CPU_IDT[IDT::INDEX_ALIGNMENT_CHECK].set_function_pointer(soft::alignment_check);
        MAIN_CPU_IDT[IDT::INDEX_ALIGNMENT_CHECK].enable();

        (&*(&raw const MAIN_CPU_IDT)).load();
    }
}

#[allow(unused)]
impl IDTEntry {
    /// 创建空白的中断描述符项
    const fn new() -> Self {
        Self {
            function_pointer_lower: 0,
            gdt_selector: 0x18,
            options: 0xF00, // 不禁用中断，无效
            function_pointer_middle: 0,
            function_pointer_high: 0,
            reserved: 0,
        }
    }

    /// 设置中断描述符中的函数指针
    /// IntFn必须是fn而不能是Fn，因为它必须是x86-interrupt调用约定（Fn是rust-call）
    fn set_function_pointer(&mut self, function: IntFn) {
        let fp = function as usize;
        self.function_pointer_lower = (fp & 0xffff) as u16;
        self.function_pointer_middle = ((fp >> 16) & 0xffff) as u16;
        self.function_pointer_high = ((fp >> 32) & 0xffff_ffff) as u32;
    }

    /// 在中断发生时禁用中断
    fn disable_interrupt(&mut self) {
        self.options |= 0x0100;
    }

    /// 启用中断描述符项
    ///
    /// 仅在启用后，CPU才会使用此中断描述符项
    fn enable(&mut self) {
        self.options |= 0x8000;
    }

    /// 设置切换栈
    ///
    /// 若不设置，中断发生后会在原栈上调用中断函数。
    /// 对于用户态程序发生的中断，应始终设置切换栈，以免破坏用户预期的栈数据（例如red zone）
    fn set_stack_index(&mut self, index: u8) {
        assert!(index < 8);
        self.options = (self.options & 0xFFF8) | (index as u16);
    }
}

#[allow(unused)]
impl IDT {
    /// Divide Error (#DE, Fault)
    ///
    /// # Source
    /// DIV and IDIV instructions
    const INDEX_DIVIDE_ERROR: usize = 0x00;

    /// Debug Exception (#DB, Trap)
    ///
    /// # Source
    /// Instruction, data, and I/O breakpoints;
    /// single-step; and others
    const INDEX_DEBUG_EXCEPTION: usize = 0x01;

    /// NMI Interrupt (NMI, Interrupt)
    ///
    /// # Source
    /// Nonmaskable external interrupt
    const INDEX_NMI_INTERRUPT: usize = 0x02;

    /// Breakpoint (#BP, Trap)
    ///
    /// # Source
    /// INT3 instruction
    const INDEX_BREAKPOINT: usize = 0x03;

    /// Overflow (#OF, Trap)
    ///
    /// # Source
    /// INTO instruction
    const INDEX_OVERFLOW: usize = 0x04;

    /// BOUND Range Exeeded (#BR, Fault)
    ///
    /// # Source
    /// BOUND instruction
    const INDEX_BOUND_RANGE_EXEEDED: usize = 0x05;

    /// Invalid Opcode (Undefined Opcode) (#UD, Fault)
    ///
    /// # Source
    /// UD instruction or reserved opcode
    const INDEX_INVALID_OPCODE: usize = 0x06;

    /// Device Not Available (No Math Coprocessor) (#NM, Fault)
    ///
    /// # Source
    /// Floating-point or WAIT/FWAIT instruction
    const INDEX_DEVICE_NOT_AVAILABLE: usize = 0x07;

    /// Double Fault (#DF, Abort)
    ///
    /// # Source
    /// Any instruction that generate an exception, an NMI, or an INTR
    ///
    /// # Error Code
    /// always zero
    const INDEX_DOUBLE_FAULT: usize = 0x08;

    /// Coprocessor Segment Overrun (Fault)
    ///
    /// # Source
    /// Floating-point instruction
    const INDEX_COMPROCESSOR_SEGMENT_OVERRUN: usize = 0x09;

    /// Invalid TSS (#TS, Fault)
    ///
    /// # Source
    /// Loading segment registers or accessing system segments
    ///
    /// # Error Code
    const INDEX_INVALID_TSS: usize = 0x0A;

    /// Segment Not Present (#NP, Fault)
    ///
    /// # Source
    /// Loading segment registers or accessing system segments
    ///
    /// # Error Code
    const INDEX_SEGMENT_NOT_PRESENT: usize = 0x0B;

    /// Stack Segment Fault (#SS, Fault)
    ///
    /// # Source
    /// Stack operations and SS register loads
    ///
    /// # Error Code
    const INDEX_STACK_SEGMENT_FAULT: usize = 0x0C;

    /// General Protection (#GP, Fault)
    ///
    /// # Source
    /// Any memory reference and other protection checks
    ///
    /// # Error Code
    const INDEX_GENERAL_PROTECTION: usize = 0x0D;

    /// Page Fault (#PF, Fault)
    ///
    /// # Source
    /// Any memory reference
    ///
    /// # Error Code
    const INDEX_PAGE_FAULT: usize = 0x0E;

    /// x87 FPU Floating-Point Error (Math Fault) (#MF, Fault)
    ///
    /// # Source
    /// x87 FPU floating-point or WAIT/FWAIT instruction
    const INDEX_FPU_FLOATING_POINT_ERROR: usize = 0x10;

    /// Alignment Check (#AC, Fault)
    ///
    /// # Source
    /// Any data reference in memory
    const INDEX_ALIGNMENT_CHECK: usize = 0x11;

    /// Machine Check (#MC, Fault)
    ///
    /// # Source
    /// Error codes (if any) and source are model dependent
    const INDEX_MACHINE_CHECK: usize = 0x12;

    /// SIMD Floating-Point Exception (#XM, Fault)
    ///
    /// # Source
    ///  SSE/SSE2/SSE3 floating-point instructions
    const INDEX_SIMD_FLOATING_POINT_EXCEPTION: usize = 0x13;

    /// Virtualization Exception (#VE, Fault)
    ///
    /// # Source
    /// EPT violations
    const INDEX_VIRTUALIZATION_EXCEPTION: usize = 0x14;

    /// Control Protection Exception (#CP, Fault)
    ///
    /// # Source
    /// RET, IRET, RSTORSSP, and SETSSBSY instructions can generate this exception.
    /// When CET indirect branch tracking is enabled, this exception can generated due to
    /// a missing ENDBRANCH instruction at target od an indirect call or jump.
    ///
    /// # Error Code
    const INDEX_CONTROL_PROTECTION_EXCEPTION: usize = 0x15;

    /// User Defined
    ///
    /// # Source
    /// External interrupts
    const INDEX_USER_DEFINED: usize = 0x20;

    /// 创建新的空白中断描述符表
    const fn new() -> Self {
        Self([IDTEntry::new(); _])
    }

    /// 将当前中断描述符表加载到CPU
    ///
    /// 当前引用必须是'static的，因为在CPU使用中断描述符表期间，对应内存数据不可被覆盖
    ///
    /// Safety: 调用方需保证调用此函数后，中断描述符表不会被释放或被覆盖
    unsafe fn load(&'static self) {
        let idtr = IDTR {
            limit: (size_of::<Self>() - 1) as u16,
            base: self,
        };

        unsafe {
            asm!("lidt [{}]", in(reg) &idtr);
        }
    }
}

impl Deref for IDT {
    type Target = [IDTEntry; 256];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for IDT {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[macro_export]
macro_rules! interrupt_handler {
    (fn $name:ident ( $stack:ident : &mut $frame:ty ) { $($body:tt)* }) => {
        #[unsafe(naked)]
        pub extern "C" fn $name() {
            // 实际执行函数要写 extern "C"，否则默认是 rust-call，而rust-call的abi并未稳定
            extern "C" fn $name($stack: &mut $frame) {
                $($body)*
            }

            ::core::arch::naked_asm!(
                // 保存通用寄存器
                "push rax", "push rbx", "push rcx", "push rdx", "push rsi", "push rdi", "push rbp",
                "push r8", "push r9", "push r10", "push r11", "push r12", "push r13", "push r14",
                "push r15",

                // 栈帧
                "mov rbp, rsp",

                // 传参
                "mov rdi, rsp",

                // 16字节对齐
                "and rsp, 0xfffffffffffffff0",

                // 调用实际处理函数
                "call {handler}",

                // 恢复栈帧
                "mov rsp, rbp",

                // 恢复通用寄存器
                "pop r15", "pop r14", "pop r13", "pop r12", "pop r11", "pop r10", "pop r9",
                "pop r8", "pop rbp", "pop rdi", "pop rsi", "pop rdx", "pop rcx", "pop rbx",
                "pop rax",

                // 中断返回
                "iretq",

                handler = sym $name
            );
        }
    };

    (#[with_error_code] fn $name:ident ( $stack:ident : &mut $frame:ty ) { $($body:tt)* }) => {
        #[unsafe(naked)]
        pub extern "C" fn $name() {
            // 实际执行函数要写 extern "C"，否则默认是 rust-call，而rust-call的abi并未稳定
            extern "C" fn $name($stack: &mut $frame) {
                $($body)*
            }

            ::core::arch::naked_asm!(
                // 保存通用寄存器
                "push rax", "push rbx", "push rcx", "push rdx", "push rsi", "push rdi", "push rbp",
                "push r8", "push r9", "push r10", "push r11", "push r12", "push r13", "push r14",
                "push r15",

                // 栈帧
                "mov rbp, rsp",

                // 传参
                "mov rdi, rsp",

                // 16字节对齐
                "and rsp, 0xfffffffffffffff0",

                // 调用实际处理函数
                "call {handler}",

                // 恢复栈帧
                "mov rsp, rbp",

                // 恢复通用寄存器
                "pop r15", "pop r14", "pop r13", "pop r12", "pop r11", "pop r10", "pop r9",
                "pop r8", "pop rbp", "pop rdi", "pop rsi", "pop rdx", "pop rcx", "pop rbx",
                "pop rax",

                // 额外弹出错误码
                "add rsp, 8",

                // 中断返回
                "iretq",

                handler = sym $name
            );
        }
    };
}
