use core::arch::asm;

use crate::{
    int::idt::{StackFrame, StackFrameWithErrorCode},
    interrupt_handler, kprintln, multitask, sync,
};

interrupt_handler! {
    fn divide_by_zero(stack: &mut StackFrame) {
        user_kill_self(stack.cs);
        panic!("#DE triggered, $rip=0x{:x}", stack.rip);
    }
}

interrupt_handler! {
    fn breakpoint(stack: &mut StackFrame) {
        kprintln!("breakpoint: {stack:x?}");
    }
}

interrupt_handler! {
    fn invalid_opcode(stack: &mut StackFrame) {
        user_kill_self(stack.cs);
        panic!("#UD triggered, $rip=0x{:x}", stack.rip);
    }
}

interrupt_handler! {
    #[with_error_code]
    fn double_fault(stack: &mut StackFrameWithErrorCode) {
        panic!("#DF triggered, stack: {stack:x?}");
    }
}

interrupt_handler! {
    #[with_error_code]
    fn invalid_tss(stack: &mut StackFrameWithErrorCode) {
        panic!("#TS triggered, $rsp=0x{:x}", stack.rsp);
    }
}

interrupt_handler! {
    #[with_error_code]
    fn segment_not_present(stack: &mut StackFrameWithErrorCode) {
        panic!("#NP triggered, $rsp=0x{:x}", stack.rsp);
    }
}

interrupt_handler! {
    #[with_error_code]
    fn stack_segment_fault(stack: &mut StackFrameWithErrorCode) {
        panic!("#SS triggered, $rsp=0x{:x}", stack.rsp);
    }
}

interrupt_handler! {
    #[with_error_code]
    fn general_protection(stack: &mut StackFrameWithErrorCode) {
        user_kill_self(stack.cs);
        panic!("#GP triggered, $rip=0x{:x}, error=0x{:x}", stack.rip, stack.error_code);
    }
}

interrupt_handler! {
    #[with_error_code]
    fn page_fault(stack: &mut StackFrameWithErrorCode) {
        user_kill_self(stack.cs);
        let fault_addr: usize;
        unsafe {
            asm!(
                "mov {}, cr2",
                out(reg) fault_addr,
                options(nostack, preserves_flags)
            );
        }
        panic!("#PF triggered, $rip=0x{:x}, fault_addr=0x{fault_addr:x}, error=0x{:x}", stack.rip, stack.error_code);
    }
}

interrupt_handler! {
    #[with_error_code]
    fn alignment_check(stack: &mut StackFrameWithErrorCode) {
        user_kill_self(stack.cs);
        panic!("#AC triggered, $rip=0x{:x}", stack.rip);
    }
}

/// 如果发生在用户态，则kill当前线程
fn user_kill_self(cs: u64) {
    if (cs & 0b11) != 0b11 {
        return;
    }

    {
        let thread_id = sync::percpu::get_current_thread_id();
        let thread = multitask::thread::get_thread(thread_id).unwrap();
        multitask::thread::stop_thread(&thread);
    }

    multitask::thread::thread_yield(true);
    unreachable!()
}
