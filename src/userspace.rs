use core::arch::asm;
use crate::gdt;

// UPDATED: Now accepts 'stack_ptr'
pub fn jump_to_code_raw(entry_ptr: u64, code_sel: u16, data_sel: u16, stack_ptr: u64) -> ! {
    unsafe {
        core::arch::asm!(
            "cli",
            "mov ds, ax",
            "mov es, ax",
            "mov fs, ax",
            "mov gs, ax",
            "push rax",
            "push rsi",
            "push 0x202",
            "push rdi",
            "push rdx",
            "iretq",
            in("ax") data_sel,
            in("rdi") code_sel,
            in("rsi") stack_ptr,
            in("rdx") entry_ptr,
            options(noreturn)
        );
    }
}

pub fn syscall_print() {
    unsafe { core::arch::asm!("int 0x80"); }
}