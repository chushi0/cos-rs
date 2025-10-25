use core::arch::asm;

use crate::{
    int::idt::{StackFrame, StackFrameWithErrorCode},
    interrupt_handler, kprintln, loop_hlt,
    sync::cli,
};

interrupt_handler! {
    fn divide_by_zero(stack: &mut StackFrame) {
        kprintln!("divide by zero: {stack:x?}");
        loop_hlt();
    }
}

interrupt_handler! {
    fn breakpoint(stack: &mut StackFrame) {
        kprintln!("breakpoint: {stack:x?}");
    }
}

interrupt_handler! {
    fn invalid_opcode(stack: &mut StackFrame) {
        kprintln!("invalid opcode: {stack:x?}");
        loop_hlt();
    }
}

interrupt_handler! {
    #[with_error_code]
    fn double_fault(stack: &mut StackFrameWithErrorCode) {
        kprintln!("double fault: {stack:x?}");
        loop_hlt();
    }
}

interrupt_handler! {
    #[with_error_code]
    fn invalid_tss(stack: &mut StackFrameWithErrorCode) {
        kprintln!("invalid tss: {stack:x?}");
        loop_hlt();
    }
}

interrupt_handler! {
    #[with_error_code]
    fn segment_not_present(stack: &mut StackFrameWithErrorCode) {
        kprintln!("segment not present: {stack:x?}");
        loop_hlt();
    }
}

interrupt_handler! {
    #[with_error_code]
    fn stack_segment_fault(stack: &mut StackFrameWithErrorCode) {
        kprintln!("stack segment fault: {stack:x?}");
        loop_hlt();
    }
}

interrupt_handler! {
    #[with_error_code]
    fn general_protection(stack: &mut StackFrameWithErrorCode) {
        kprintln!("general protection: {stack:x?}");
        loop_hlt();
    }
}

interrupt_handler! {
    #[with_error_code]
    fn page_fault(stack: &mut StackFrameWithErrorCode) {
        unsafe{cli();}
        kprintln!("page fault: {stack:x?}");
        let fault_addr: usize;
        unsafe {
            asm!(
                "mov {}, cr2",
                out(reg) fault_addr,
                options(nostack, preserves_flags)
            );
        }
        kprintln!("fault addr: 0x{:x}", fault_addr);
        loop_hlt();
    }
}

interrupt_handler! {
    #[with_error_code]
    fn alignment_check(stack: &mut StackFrameWithErrorCode) {
        kprintln!("alignment check: {stack:x?}");
        loop_hlt();
    }
}
