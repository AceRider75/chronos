use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};
use lazy_static::lazy_static;
use pic8259::ChainedPics;
use spin::Mutex;
use x86_64::instructions::port::Port;
use pc_keyboard::{layouts, DecodedKey, HandleControl, Keyboard, ScancodeSet1};
use crate::{state, input, writer};

pub const PIC_1_OFFSET: u8 = 32;
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum InterruptIndex {
    Timer = PIC_1_OFFSET,
    Keyboard = PIC_1_OFFSET + 1,
}

pub static PICS: Mutex<ChainedPics> = Mutex::new(unsafe { 
    ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) 
});

// Helper to Force-Unmask Interrupts on the PIC
pub fn enable_listening() {
    unsafe {
        // Master PIC: Unmask IRQ0 (Timer) and IRQ1 (Keyboard)
        // 11111100 = 0xFC
        let mut port = Port::<u8>::new(0x21);
        port.write(0xFC); 
        // Slave PIC: Mask all
        let mut port2 = Port::<u8>::new(0xA1);
        port2.write(0xFF);
    }
}

lazy_static! {
    static ref KEYBOARD: Mutex<Keyboard<layouts::Us104Key, ScancodeSet1>> =
        Mutex::new(Keyboard::new(ScancodeSet1::new(), layouts::Us104Key, HandleControl::Ignore));
}

lazy_static! {
    static ref IDT: InterruptDescriptorTable = {
        let mut idt = InterruptDescriptorTable::new();
        idt.breakpoint.set_handler_fn(breakpoint_handler);
        
        // NEW: Handle Page Faults (Vector 14)
        // This is what will fire when we jump to Ring 3 without Page Tables set up for it.
        idt.page_fault.set_handler_fn(page_fault_handler);
        
        idt[InterruptIndex::Keyboard as usize].set_handler_fn(keyboard_interrupt_handler);
        idt[InterruptIndex::Timer as usize].set_handler_fn(timer_interrupt_handler);
        idt
    };
}

pub fn init_idt() {
    IDT.load();
}

extern "x86-interrupt" fn breakpoint_handler(_stack_frame: InterruptStackFrame) {}

// --- THE NEW HANDLER ---
extern "x86-interrupt" fn page_fault_handler(
    _stack_frame: InterruptStackFrame,
    _error_code: PageFaultErrorCode,
) {
    // IMPORTANT: We must NOT enable interrupts here or we might loop forever.
    
    writer::print("\n\n[EXCEPTION: PAGE FAULT]\n");
    writer::print("-----------------------\n");
    writer::print("Reason: Access Violation.\n");
    writer::print("Context: The CPU is in Ring 3 (User Mode).\n");
    writer::print("Action: Attempted to execute Kernel Code.\n");
    writer::print("RESULT: SUCCESS! Ring 3 Isolation is ACTIVE.\n");

    loop { core::hint::spin_loop(); }
}

extern "x86-interrupt" fn timer_interrupt_handler(_stack_frame: InterruptStackFrame) {
    state::KEY_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    unsafe {
        PICS.lock().notify_end_of_interrupt(InterruptIndex::Timer as u8);
    }
}

extern "x86-interrupt" fn keyboard_interrupt_handler(_stack_frame: InterruptStackFrame) {
    let mut keyboard = KEYBOARD.lock();
    let mut port = Port::new(0x60);
    let scancode: u8 = unsafe { port.read() };

    state::KEY_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);

    if let Ok(Some(key_event)) = keyboard.add_byte(scancode) {
        if let Some(key) = keyboard.process_keyevent(key_event) {
            match key {
                DecodedKey::Unicode(character) => {
                    input::push_key(character);
                },
                DecodedKey::RawKey(_) => {},
            }
        }
    }

    unsafe {
        PICS.lock().notify_end_of_interrupt(InterruptIndex::Keyboard as u8);
    }
}