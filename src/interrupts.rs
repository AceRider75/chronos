
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};
use x86_64::PrivilegeLevel; // NEW IMPORT
use lazy_static::lazy_static;
use pic8259::ChainedPics;
use spin::Mutex;
use x86_64::instructions::port::Port;
use pc_keyboard::{layouts, DecodedKey, HandleControl, Keyboard, ScancodeSet1};
use crate::{state, input, writer};

// --- CONFIGURATION ---
pub const PIC_1_OFFSET: u8 = 32;
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;
pub const SYSCALL_IRQ: u8 = 0x80; // Vector 128 (Linux legacy syscall)

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
        Mutex::new(Keyboard::new(ScancodeSet1::new(), layouts::Us104Key, HandleControl::Ignore));
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
        // We set the Privilege Level to Ring 3.
        // This is the GATE. It allows Ring 3 code to jump to this specific Ring 0 function.
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
    
    // Read the address that caused the crash
    let cr2 = x86_64::registers::control::Cr2::read();

    writer::print("\n\n[EXCEPTION: PAGE FAULT]\n");
    writer::print("-----------------------\n");
    
    // Print the address in Hex
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
// This runs when User Mode calls 'int 0x80'
extern "x86-interrupt" fn syscall_handler(_stack_frame: InterruptStackFrame) {
    // Enable interrupts briefly so printing doesn't deadlock if the writer is busy
    x86_64::instructions::interrupts::enable();

    writer::print("\n----------------------------------------\n");
    writer::print("[KERNEL] System Call Received (Vector 80)\n");
    writer::print("[KERNEL] Origin: User Mode (Ring 3)\n");
    writer::print("[KERNEL] Action: User requested 'Hello World'\n");
    writer::print("----------------------------------------\n");

    // In a real OS, we would look at registers (RAX) to decide what function to run.
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

extern "x86-interrupt" fn mouse_interrupt_handler(_stack_frame: InterruptStackFrame) {
    crate::mouse::handle_interrupt();
    unsafe {
        // Since Mouse is on Slave PIC (IRQ 12), we must notify BOTH PICs
        PICS.lock().notify_end_of_interrupt(InterruptIndex::Mouse as u8);
    }
}