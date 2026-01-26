use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};
use x86_64::PrivilegeLevel;
use lazy_static::lazy_static;
use pic8259::ChainedPics;
use spin::Mutex;
use x86_64::instructions::port::Port;
use pc_keyboard::{layouts, DecodedKey, HandleControl, Keyboard, ScancodeSet1};
use x86_64::VirtAddr;
use crate::{state, input, writer, gdt};
use core::sync::atomic::Ordering;

// --- CONFIGURATION ---
pub const PIC_1_OFFSET: u8 = 32;
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;
pub const SYSCALL_IRQ: u8 = 0x80;

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum InterruptIndex {
    Timer = PIC_1_OFFSET,
    Keyboard = PIC_1_OFFSET + 1,
    Mouse = PIC_2_OFFSET + 4,
}

pub static PICS: Mutex<ChainedPics> = Mutex::new(unsafe { 
    ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) 
});

pub fn enable_listening() {
    unsafe {
        let mut port = Port::<u8>::new(0x21);
        port.write(0xF8); 
        let mut port2 = Port::<u8>::new(0xA1);
        port2.write(0xEF);
    }
}

lazy_static! {
    static ref KEYBOARD: Mutex<Keyboard<layouts::Us104Key, ScancodeSet1>> =
        Mutex::new(Keyboard::new(ScancodeSet1::new(), layouts::Us104Key, HandleControl::MapLettersToUnicode));
}

// --- IDT SETUP ---
lazy_static! {
    static ref IDT: InterruptDescriptorTable = {
        let mut idt = InterruptDescriptorTable::new();
        idt.breakpoint.set_handler_fn(breakpoint_handler);
        idt.page_fault.set_handler_fn(page_fault_handler);
        
        idt[InterruptIndex::Keyboard as usize].set_handler_fn(keyboard_interrupt_handler);
        idt[InterruptIndex::Mouse as usize].set_handler_fn(mouse_interrupt_handler);
        idt[InterruptIndex::Timer as usize].set_handler_fn(timer_interrupt_handler);
        
        // SYSTEM CALL (0x80)
        idt[SYSCALL_IRQ as usize]
            .set_handler_fn(syscall_handler)
            .set_privilege_level(PrivilegeLevel::Ring3);
        
        idt
    };
}

pub fn init_idt() {
    IDT.load();
}

// --- HANDLERS ---

extern "x86-interrupt" fn breakpoint_handler(_stack_frame: InterruptStackFrame) {}

extern "x86-interrupt" fn page_fault_handler(
    _stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    x86_64::instructions::interrupts::disable();
    
    let cr2 = x86_64::registers::control::Cr2::read();

    writer::print("\n\n[EXCEPTION: PAGE FAULT]\n");
    writer::print("-----------------------\n");
    
    use alloc::format;
    writer::print(&format!("Accessed Address (CR2): {:x}\n", cr2));
    
    if error_code.contains(PageFaultErrorCode::PROTECTION_VIOLATION) {
        writer::print("Reason: PROTECTION VIOLATION (Ring 3 blocked)\n");
    } else {
        writer::print("Reason: PAGE NOT PRESENT (Mapping missing)\n");
    }
    
    writer::print("SYSTEM HALTED.\n");
    loop { core::hint::spin_loop(); }
}

// THE SYSCALL HANDLER
extern "x86-interrupt" fn syscall_handler(stack_frame: InterruptStackFrame) {
    x86_64::instructions::interrupts::enable();

    // 1. Get Video Info
    let video_addr = state::VIDEO_PTR.load(Ordering::Relaxed);
    let width = state::SCREEN_WIDTH.load(Ordering::Relaxed);
    let height = state::SCREEN_HEIGHT.load(Ordering::Relaxed);

    // 3. (Removed Blue Box Drawing)
    // The shell will handle the success message upon resume.

    // Return to shell by manually switching stack and jumping
    // We cannot rely on IRETQ to switch stacks when returning to Ring 0.
    
    let rsp = crate::shell::KERNEL_RSP.load(Ordering::Relaxed);
    let entry = crate::shell::resume_shell as *const () as u64;

    unsafe {
        core::arch::asm!(
            "mov rsp, {0}",   // 1. Restore Kernel Stack
            "sti",            // 2. Enable Interrupts
            "jmp {1}",        // 3. Jump to Shell
            in(reg) rsp,
            in(reg) entry,
            options(noreturn)
        );
    }
}

extern "x86-interrupt" fn timer_interrupt_handler(_stack_frame: InterruptStackFrame) {
    state::KEY_COUNT.fetch_add(1, Ordering::Relaxed);
    unsafe { PICS.lock().notify_end_of_interrupt(InterruptIndex::Timer as u8); }
}

extern "x86-interrupt" fn keyboard_interrupt_handler(_stack_frame: InterruptStackFrame) {
    let mut keyboard = KEYBOARD.lock();
    let mut port = Port::new(0x60);
    let scancode: u8 = unsafe { port.read() };

    state::KEY_COUNT.fetch_add(1, Ordering::Relaxed);

    if let Ok(Some(key_event)) = keyboard.add_byte(scancode) {
        if let Some(key) = keyboard.process_keyevent(key_event) {
            match key {
                DecodedKey::Unicode(character) => { input::push_key(character); },
                DecodedKey::RawKey(k) => {
                    use pc_keyboard::KeyCode;
                    match k {
                        KeyCode::ArrowUp => input::push_key('\u{E000}'),
                        KeyCode::ArrowDown => input::push_key('\u{E001}'),
                        KeyCode::ArrowLeft => input::push_key('\u{E002}'),
                        KeyCode::ArrowRight => input::push_key('\u{E003}'),
                        _ => {}
                    }
                },
            }
        }
    }
    unsafe { PICS.lock().notify_end_of_interrupt(InterruptIndex::Keyboard as u8); }
}

extern "x86-interrupt" fn mouse_interrupt_handler(_stack_frame: InterruptStackFrame) {
    crate::mouse::handle_interrupt();
    unsafe {
        PICS.lock().notify_end_of_interrupt(InterruptIndex::Mouse as u8);
    }
}