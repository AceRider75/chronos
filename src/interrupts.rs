use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};
use lazy_static::lazy_static;
use pic8259::ChainedPics;
use spin::Mutex;
use x86_64::instructions::port::Port;
use pc_keyboard::{layouts, DecodedKey, HandleControl, Keyboard, ScancodeSet1};
use crate::{state, input, writer};

// --- CONFIGURATION ---
pub const PIC_1_OFFSET: u8 = 32;
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;
pub const SYSCALL_IRQ: u8 = 0x80; // 128

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum InterruptIndex {
    Timer = PIC_1_OFFSET,
    Keyboard = PIC_1_OFFSET + 1,
}

pub static PICS: Mutex<ChainedPics> = Mutex::new(unsafe { 
    ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) 
});

// Helper to Unmask Interrupts
pub fn enable_listening() {
    unsafe {
        let mut port = Port::<u8>::new(0x21);
        port.write(0xFC); // Unmask 0 and 1
        let mut port2 = Port::<u8>::new(0xA1);
        port2.write(0xFF);
    }
}

lazy_static! {
    static ref KEYBOARD: Mutex<Keyboard<layouts::Us104Key, ScancodeSet1>> =
        Mutex::new(Keyboard::new(ScancodeSet1::new(), layouts::Us104Key, HandleControl::Ignore));
}

// --- IDT SETUP ---
lazy_static! {
    static ref IDT: InterruptDescriptorTable = {
        let mut idt = InterruptDescriptorTable::new();
        idt.breakpoint.set_handler_fn(breakpoint_handler);
        
        // CRITICAL: Handle Memory Permission Errors
        idt.page_fault.set_handler_fn(page_fault_handler);
        
        // Hardware Interrupts
        idt[InterruptIndex::Keyboard as usize].set_handler_fn(keyboard_interrupt_handler);
        idt[InterruptIndex::Timer as usize].set_handler_fn(timer_interrupt_handler);
        
        // System Call (Future Proofing)
        // Note: To call this from Ring 3, we would need to set DPL=3 options.
        // For now, it just sits here ready.
        idt[SYSCALL_IRQ as usize].set_handler_fn(syscall_handler);
        
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
    // If code reaches here, it means the VMM Memory Unlocking FAILED.
    // The CPU blocked the User App from running.
    
    // Disable interrupts to stop the bleeding
    x86_64::instructions::interrupts::disable();
    
    writer::print("\n\n[EXCEPTION: PAGE FAULT]\n");
    writer::print("-----------------------\n");
    // We can check if it was a protection violation (P) or missing page (not P)
    if error_code.contains(PageFaultErrorCode::PROTECTION_VIOLATION) {
        writer::print("Reason: PROTECTION VIOLATION\n");
        writer::print("Explanation: Ring 3 tried to touch Kernel Memory, and the 'USER' flag was NOT set.\n");
    } else {
        writer::print("Reason: PAGE NOT PRESENT\n");
    }
    
    let cr2 = x86_64::registers::control::Cr2::read();
    // In a real OS, we would print the address in CR2 here using a hex formatter
    writer::print("Crash Address: CR2 Register\n");
    
    writer::print("SYSTEM HALTED.\n");
    loop { core::hint::spin_loop(); }
}

extern "x86-interrupt" fn syscall_handler(_stack_frame: InterruptStackFrame) {
    writer::print("[SYSCALL] Hello from Kernel Mode!\n");
}

extern "x86-interrupt" fn timer_interrupt_handler(_stack_frame: InterruptStackFrame) {
    state::KEY_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    unsafe { PICS.lock().notify_end_of_interrupt(InterruptIndex::Timer as u8); }
}

extern "x86-interrupt" fn keyboard_interrupt_handler(_stack_frame: InterruptStackFrame) {
    let mut keyboard = KEYBOARD.lock();
    let mut port = Port::new(0x60);
    let scancode: u8 = unsafe { port.read() };

    state::KEY_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);

    if let Ok(Some(key_event)) = keyboard.add_byte(scancode) {
        if let Some(key) = keyboard.process_keyevent(key_event) {
            match key {
                DecodedKey::Unicode(character) => { input::push_key(character); },
                DecodedKey::RawKey(_) => {},
            }
        }
    }
    unsafe { PICS.lock().notify_end_of_interrupt(InterruptIndex::Keyboard as u8); }
}