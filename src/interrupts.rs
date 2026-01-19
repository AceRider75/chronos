use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame};
use lazy_static::lazy_static;

lazy_static! {
    static ref IDT: InterruptDescriptorTable = {
        let mut idt = InterruptDescriptorTable::new();
        idt.breakpoint.set_handler_fn(breakpoint_handler);
        idt
    };
}

pub fn init_idt() {
    IDT.load();
}

// FIX: Added underscore to _stack_frame
extern "x86-interrupt" fn breakpoint_handler(_stack_frame: InterruptStackFrame) {
    // The handler does nothing for now, just returns.
}