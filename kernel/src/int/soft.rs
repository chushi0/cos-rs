use crate::{
    int::{StackFrame, StackFrameWithErrorCode},
    interrupt_handler, kprintln, loop_hlt,
};

interrupt_handler! {
    fn divide_by_zero(stack: &mut StackFrame) {
        kprintln!("divide by zero: {stack:?}");
        loop_hlt();
    }
}

interrupt_handler! {
    fn breakpoint(stack: &mut StackFrame) {
        kprintln!("breakpoint: {stack:?}");
    }
}

interrupt_handler! {
    fn invalid_opcode(stack: &mut StackFrame) {
        kprintln!("invalid opcode: {stack:?}");
        loop_hlt();
    }
}

interrupt_handler! {
    fn double_fault(stack: &mut StackFrameWithErrorCode) {
        kprintln!("double fault: {stack:?}");
        loop_hlt();
    }
}

interrupt_handler! {
    fn invalid_tss(stack: &mut StackFrameWithErrorCode) {
        kprintln!("invalid tss: {stack:?}");
        loop_hlt();
    }
}

interrupt_handler! {
    fn segment_not_present(stack: &mut StackFrameWithErrorCode) {
        kprintln!("segment not present: {stack:?}");
        loop_hlt();
    }
}

interrupt_handler! {
    fn stack_segment_fault(stack: &mut StackFrameWithErrorCode) {
        kprintln!("stack segment fault: {stack:?}");
        loop_hlt();
    }
}

interrupt_handler! {
    fn general_protection(stack: &mut StackFrameWithErrorCode) {
        kprintln!("general protection: {stack:?}");
        loop_hlt();
    }
}

interrupt_handler! {
    fn page_fault(stack: &mut StackFrameWithErrorCode) {
        kprintln!("page fault: {stack:?}");
        loop_hlt();
    }
}

interrupt_handler! {
    fn alignment_check(stack: &mut StackFrameWithErrorCode) {
        kprintln!("alignment check: {stack:?}");
        loop_hlt();
    }
}
