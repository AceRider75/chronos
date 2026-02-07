use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};
use x86_64::PrivilegeLevel;
use lazy_static::lazy_static;
use pic8259::ChainedPics;
use spin::Mutex;
use x86_64::instructions::port::Port;
use pc_keyboard::{layouts, DecodedKey, HandleControl, Keyboard, ScancodeSet1};
use x86_64::VirtAddr;
use crate::{state, input, writer, gdt, scheduler};
use core::sync::atomic::{Ordering, AtomicBool};
use crate::scheduler::{TaskContext, SCHEDULER, SCHEDULER_CONTEXT};

static CTRL_PRESSED: AtomicBool = AtomicBool::new(false);
static SHIFT_PRESSED: AtomicBool = AtomicBool::new(false);

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

pub fn init_pit() {
    let divisor: u16 = 11931; // ~100Hz
    unsafe {
        Port::new(0x43).write(0x36u8);
        Port::new(0x40).write((divisor & 0xFF) as u8);
        Port::new(0x40).write((divisor >> 8) as u8);
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
        idt.general_protection_fault.set_handler_fn(general_protection_fault_handler);
        
        unsafe {
            idt.double_fault.set_handler_fn(double_fault_handler)
                .set_stack_index(gdt::DOUBLE_FAULT_IST_INDEX);
        }
        
        unsafe {
            idt[InterruptIndex::Keyboard as usize]
                .set_handler_fn(keyboard_interrupt_handler)
                .set_stack_index(gdt::INTERRUPT_IST_INDEX);
                
            idt[InterruptIndex::Mouse as usize]
                .set_handler_fn(mouse_interrupt_handler)
                .set_stack_index(gdt::INTERRUPT_IST_INDEX);

            idt[InterruptIndex::Timer as usize]
                .set_handler_fn(core::mem::transmute(timer_interrupt_handler as *const ()))
                .set_stack_index(gdt::INTERRUPT_IST_INDEX);
            
            // SYSTEM CALL (0x80)
            idt[SYSCALL_IRQ as usize]
                .set_handler_fn(core::mem::transmute(syscall_handler as *const ()))
                .set_privilege_level(PrivilegeLevel::Ring3)
                .set_stack_index(gdt::INTERRUPT_IST_INDEX);
        }
        
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
    writer::print(&format!("Instruction Pointer (RIP): {:x}\n", _stack_frame.instruction_pointer.as_u64()));
    
    if error_code.contains(PageFaultErrorCode::PROTECTION_VIOLATION) {
        writer::print("Reason: PROTECTION VIOLATION (Ring 3 blocked)\n");
    } else {
        writer::print("Reason: PAGE NOT PRESENT (Mapping missing)\n");
    }
    
    writer::print("SYSTEM HALTED.\n");
    crate::serial_print!("[EXCEPTION: PAGE FAULT] CR2={:x} RIP={:x}\n", cr2, _stack_frame.instruction_pointer.as_u64());
    if error_code.contains(PageFaultErrorCode::PROTECTION_VIOLATION) {
        crate::serial_print!("Reason: PROTECTION VIOLATION\n");
    } else {
        crate::serial_print!("Reason: PAGE NOT PRESENT\n");
    }
    loop { core::hint::spin_loop(); }
}

extern "x86-interrupt" fn general_protection_fault_handler(
    _stack_frame: InterruptStackFrame,
    error_code: u64,
) {
    x86_64::instructions::interrupts::disable();
    crate::serial_print!("\n[EXCEPTION: GENERAL PROTECTION FAULT]\n");
    crate::serial_print!("Error Code: {}\n", error_code);
    crate::serial_print!("RIP: {:x}\n", _stack_frame.instruction_pointer.as_u64());
    crate::serial_print!("SYSTEM HALTED.\n");
    loop { core::hint::spin_loop(); }
}

extern "x86-interrupt" fn double_fault_handler(
    _stack_frame: InterruptStackFrame,
    _error_code: u64,
) -> ! {
    crate::serial_print!("\n[EXCEPTION: DOUBLE FAULT]\n");
    crate::serial_print!("RIP: {:x}\n", _stack_frame.instruction_pointer.as_u64());
    crate::serial_print!("SYSTEM HALTED.\n");
    loop { core::hint::spin_loop(); }
}

#[unsafe(naked)]
pub extern "C" fn timer_interrupt_handler() {
    core::arch::naked_asm!(
        // CPU already pushed: ss, rsp, rflags, cs, rip (at higher addresses)
        // We need r15 at RSP+0, r14 at RSP+8, ..., rax at RSP+112
        // So push rax first (ends up at RSP+112), then down to r15 (ends up at RSP+0)
        "push rax",
        "push rbx",
        "push rcx",
        "push rdx",
        "push rsi",
        "push rdi",
        "push rbp",
        "push r8",
        "push r9",
        "push r10",
        "push r11",
        "push r12",
        "push r13",
        "push r14",
        "push r15",
        "mov rdi, rsp",
        "call {handle_timer}",
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop r11",
        "pop r10",
        "pop r9",
        "pop r8",
        "pop rbp",
        "pop rdi",
        "pop rsi",
        "pop rdx",
        "pop rcx",
        "pop rbx",
        "pop rax",
        "iretq",
        handle_timer = sym handle_timer_preemption,
    );
}

extern "C" fn handle_timer_preemption(context: *mut TaskContext) {
    state::KEY_COUNT.fetch_add(1, Ordering::Relaxed);

    
    let mut sched = SCHEDULER.lock();
    if let Some(idx) = sched.current_task_idx {
        unsafe {
            // 1. Save Task Context
            sched.tasks[idx].context = *context;
            // 2. Load Scheduler Context (Swap!) with interrupts enabled
            *context = SCHEDULER_CONTEXT;
            (*context).rflags |= 0x200; // Force IF bit
        }
    }

    unsafe { PICS.lock().notify_end_of_interrupt(InterruptIndex::Timer as u8); }
}

extern "x86-interrupt" fn keyboard_interrupt_handler(_stack_frame: InterruptStackFrame) {
    let mut keyboard = KEYBOARD.lock();
    let mut port = Port::new(0x60);
    let scancode: u8 = unsafe { port.read() };

    state::KEY_COUNT.fetch_add(1, Ordering::Relaxed);

    if let Ok(Some(key_event)) = keyboard.add_byte(scancode) {
        use pc_keyboard::KeyCode;
        
        // Track Modifiers
        match key_event.code {
            KeyCode::LControl | KeyCode::RControl => {
                CTRL_PRESSED.store(key_event.state == pc_keyboard::KeyState::Down, Ordering::Relaxed);
            }
            KeyCode::LShift | KeyCode::RShift => {
                SHIFT_PRESSED.store(key_event.state == pc_keyboard::KeyState::Down, Ordering::Relaxed);
            }
            _ => {}
        }

        let ctrl = CTRL_PRESSED.load(Ordering::Relaxed);
        let shift = SHIFT_PRESSED.load(Ordering::Relaxed);

        if ctrl && shift && key_event.state == pc_keyboard::KeyState::Down {
            match key_event.code {
                KeyCode::C => { input::push_key('\u{E004}'); },
                KeyCode::V => { input::push_key('\u{E005}'); },
                _ => {
                    if let Some(key) = keyboard.process_keyevent(key_event) {
                        match key {
                            DecodedKey::Unicode(character) => { input::push_key(character); },
                            DecodedKey::RawKey(k) => {
                                match k {
                                    KeyCode::ArrowUp => input::push_key('\u{E000}'),
                                    KeyCode::ArrowDown => input::push_key('\u{E001}'),
                                    KeyCode::ArrowLeft => input::push_key('\u{E002}'),
                                    KeyCode::ArrowRight => input::push_key('\u{E003}'),
                                    KeyCode::Delete => input::push_key('\u{E006}'),
                                    _ => {}
                                }
                            },
                        }
                    }
                }
            }
        } else {
            if let Some(key) = keyboard.process_keyevent(key_event) {
                match key {
                    DecodedKey::Unicode(character) => { input::push_key(character); },
                    DecodedKey::RawKey(k) => {
                        match k {
                            KeyCode::ArrowUp => input::push_key('\u{E000}'),
                            KeyCode::ArrowDown => input::push_key('\u{E001}'),
                            KeyCode::ArrowLeft => input::push_key('\u{E002}'),
                            KeyCode::ArrowRight => input::push_key('\u{E003}'),
                            KeyCode::Delete => input::push_key('\u{E006}'),
                            _ => {}
                        }
                    },
                }
            }
        }
    }
    unsafe { PICS.lock().notify_end_of_interrupt(InterruptIndex::Keyboard as u8); }
}

#[unsafe(naked)]
pub extern "C" fn syscall_handler() {
    core::arch::naked_asm!(
        "push rax",
        "push rbx",
        "push rcx",
        "push rdx",
        "push rsi",
        "push rdi",
        "push rbp",
        "push r8",
        "push r9",
        "push r10",
        "push r11",
        "push r12",
        "push r13",
        "push r14",
        "push r15",
        "mov rdi, rsp",
        "call {handle_syscall}",
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop r11",
        "pop r10",
        "pop r9",
        "pop r8",
        "pop rbp",
        "pop rdi",
        "pop rsi",
        "pop rdx",
        "pop rcx",
        "pop rbx",
        "pop rax",
        "iretq",
        handle_syscall = sym handle_syscall_rust,
    );
}

extern "C" fn handle_syscall_rust(context: *mut TaskContext) {
    let rax = unsafe { (*context).rax };
    let rdi = unsafe { (*context).rdi };
    let rsi = unsafe { (*context).rsi };

    match rax {
        1 => { // print
            let ptr = rdi as *const u8;
            let len = rsi as usize;
            let s = unsafe { core::str::from_utf8_unchecked(core::slice::from_raw_parts(ptr, len)) };
            writer::print(s);
            crate::serial_print!("{}", s);
        }
        2 => { // exit
            let mut sched = SCHEDULER.lock();
            if let Some(idx) = sched.current_task_idx {
                sched.tasks.remove(idx);
                sched.current_task_idx = None;
                // Switch back to scheduler with interrupts enabled!
                unsafe { 
                    *context = SCHEDULER_CONTEXT;
                    (*context).rflags |= 0x200; // Force IF bit
                }
            }
        }
        3 => { // yield
            let mut sched = SCHEDULER.lock();
            if let Some(idx) = sched.current_task_idx {
                // 1. Save Task Context!
                sched.tasks[idx].context = unsafe { *context };
                
                // 2. Switch back to scheduler with interrupts enabled!
                unsafe { 
                    *context = SCHEDULER_CONTEXT;
                    (*context).rflags |= 0x200; // Force IF bit
                }
            }
        }
        _ => {}
    }
}

extern "x86-interrupt" fn mouse_interrupt_handler(_stack_frame: InterruptStackFrame) {
    crate::mouse::handle_interrupt();
    unsafe {
        PICS.lock().notify_end_of_interrupt(InterruptIndex::Mouse as u8);
    }
}