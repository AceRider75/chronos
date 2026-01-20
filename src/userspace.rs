use core::arch::asm;
use crate::gdt;

// UPDATED: Now accepts 'stack_ptr'
pub fn jump_to_code(function_ptr: fn() -> !, code_sel: u16, data_sel: u16, stack_ptr: u64) -> ! {
    unsafe {
        asm!(
            "cli",
            "mov ds, ax",
            "mov es, ax",
            "mov fs, ax",
            "mov gs, ax",
            "push rax",          // SS
            "push rsi",          // RSP (We use the passed stack_ptr)
            "push 0x202",        // RFLAGS (Interrupts Enabled)
            "push rdi",          // CS
            "push rdx",          // RIP
            "iretq",
            in("ax") data_sel,
            in("rdi") code_sel,
            in("rsi") stack_ptr, // Input register for stack
            in("rdx") function_ptr,
            options(noreturn)
        );
    }
}

pub fn syscall_print() {
    unsafe { core::arch::asm!("int 0x80"); }
}