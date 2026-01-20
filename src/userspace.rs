// src/userspace.rs
use core::arch::asm;
use crate::gdt;

// We renamed this to 'jump_to_code' to be more generic
pub fn jump_to_code(function_ptr: fn() -> !, code_sel: u16, data_sel: u16) -> ! {
    static mut STACK: [u8; 4096] = [0; 4096];
    let stack_ptr = unsafe { STACK.as_ptr() as usize + 4096 };

    unsafe {
        asm!(
            "cli",
            "mov ds, ax",
            "mov es, ax",
            "mov fs, ax",
            "mov gs, ax",
            "push rax",          // SS
            "push rsi",          // RSP
            "push 0x202",        // RFLAGS (Interrupts Enabled!)
            "push rdi",          // CS
            "push rdx",          // RIP
            "iretq",
            in("ax") data_sel,
            in("rdi") code_sel,
            in("rsi") stack_ptr,
            in("rdx") function_ptr,
            options(noreturn)
        );
    }
}