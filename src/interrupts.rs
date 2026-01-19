// src/interrupts.rs
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame};
use lazy_static::lazy_static;
use pic8259::ChainedPics;
use spin::Mutex;
use x86_64::instructions::port::Port;
use pc_keyboard::{layouts, DecodedKey, HandleControl, Keyboard, ScancodeSet1};
use crate::state;

pub const PIC_1_OFFSET: u8 = 32;
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum InterruptIndex {
    Timer = PIC_1_OFFSET,      // 32
    Keyboard = PIC_1_OFFSET + 1, // 33
}

pub static PICS: Mutex<ChainedPics> = Mutex::new(unsafe { 
    ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) 
});

lazy_static! {
    static ref KEYBOARD: Mutex<Keyboard<layouts::Us104Key, ScancodeSet1>> =
        Mutex::new(Keyboard::new(ScancodeSet1::new(), layouts::Us104Key, HandleControl::Ignore));
}

lazy_static! {
    static ref IDT: InterruptDescriptorTable = {
        let mut idt = InterruptDescriptorTable::new();
        idt.breakpoint.set_handler_fn(breakpoint_handler);
        
        // REGISTER KEYBOARD (33)
        idt[InterruptIndex::Keyboard as usize].set_handler_fn(keyboard_interrupt_handler);
        
        // REGISTER TIMER (32) - NEW!
        idt[InterruptIndex::Timer as usize].set_handler_fn(timer_interrupt_handler);
        
        idt
    };
}

pub fn init_idt() {
    IDT.load();
}

extern "x86-interrupt" fn breakpoint_handler(_stack_frame: InterruptStackFrame) {}

// NEW: The Heartbeat Handler
extern "x86-interrupt" fn timer_interrupt_handler(_stack_frame: InterruptStackFrame) {
    // 1. Increment the counter automatically (Strobe light)
    state::KEY_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);

    // 2. Tell PIC we are done
    unsafe {
        PICS.lock().notify_end_of_interrupt(InterruptIndex::Timer as u8);
    }
}

extern "x86-interrupt" fn keyboard_interrupt_handler(_stack_frame: InterruptStackFrame) {
    let mut keyboard = KEYBOARD.lock();
    let mut port = Port::new(0x60);
    let scancode: u8 = unsafe { port.read() };

    // Increment counter on keypress too
    state::KEY_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);

    if let Ok(Some(key_event)) = keyboard.add_byte(scancode) {
        if let Some(key) = keyboard.process_keyevent(key_event) {
            match key {
                DecodedKey::Unicode(character) => {
                    match character {
                        '=' | '+' => state::adjust_budget(1_000_000), 
                        '-' | '_' => state::adjust_budget(-1_000_000),
                        _ => {},
                    }
                },
                DecodedKey::RawKey(_) => {},
            }
        }
    }

    unsafe {
        PICS.lock().notify_end_of_interrupt(InterruptIndex::Keyboard as u8);
    }
}

// ... existing code ...

// NEW: Helper to force-unmask interrupts
pub fn enable_listening() {
    unsafe {
        // 0x21 is the Master PIC Data Port
        // We want to write a "Mask".
        // 0 = Listen, 1 = Ignore.
        // Bit 0 = Timer (IRQ0)
        // Bit 1 = Keyboard (IRQ1)
        // 11111100 in binary is 0xFC.
        // This tells PIC: "Listen to Timer and Keyboard, Ignore everything else."
        let mut port = Port::<u8>::new(0x21);
        port.write(0xFC); 
        
        // 0xA1 is Slave PIC. Mask everything (0xFF).
        let mut port2 = Port::<u8>::new(0xA1);
        port2.write(0xFF);
    }
}