use crate::{input, writer, fs, userspace, gdt, memory, state}; 
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;
use lazy_static::lazy_static;
use core::sync::atomic::Ordering;

// DEFINE A STATIC STACK FOR THE USER APP
// 4KB Stack size, aligned to page boundary
#[repr(align(4096))]
struct UserStack([u8; 4096]);
static mut USER_STACK: UserStack = UserStack([0; 4096]);

pub struct Shell {
    command_buffer: String,
}

impl Shell {
    pub fn new() -> Self {
        Shell { command_buffer: String::new() }
    }

    pub fn run(&mut self) {
        // ... (Keep existing run loop logic unchanged) ...
        while let Some(c) = input::pop_key() {
            match c {
                '\n' => {
                    writer::print("\n");
                    self.execute_command();
                    self.command_buffer.clear();
                    writer::print("> "); 
                }
                '\x08' => {
                    if !self.command_buffer.is_empty() {
                        self.command_buffer.pop();
                        writer::print("\x08"); 
                    }
                }
                _ => {
                    self.command_buffer.push(c);
                    let mut s = String::new();
                    s.push(c);
                    writer::print(&s);
                }
            }
        }
    }

    fn execute_command(&self) {
        let cmd = self.command_buffer.trim();
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        if parts.is_empty() { return; }

        match parts[0] {
            "help" => writer::print("Commands: help, ver, clear, ls, cat, exec\n"),
            "ver" => writer::print("Chronos OS v0.9\n"),
            "clear" => {
                if let Some(w) = writer::WRITER.lock().as_mut() {
                    w.clear();
                    w.cursor_y = 10;
                }
            },
            "ls" => {
                writer::print("--- Files ---\n");
                for file in fs::list_files() {
                    writer::print("- ");
                    writer::print(&file.name);
                    writer::print("\n");
                }
            },
            "cat" => {
                if parts.len() < 2 { writer::print("Usage: cat <file>\n"); return; }
                if let Some(content) = fs::read_file(parts[1]) {
                    writer::print(&content);
                    writer::print("\n");
                } else {
                    writer::print("File not found.\n");
                }
            },
            "exec" => {
                writer::print("[SHELL] Launching User Mode Application...\n");
                
                fn my_user_app() -> ! {
                    // Trigger Syscall
                    unsafe { core::arch::asm!("int 0x80"); }
                    loop { core::hint::spin_loop(); }
                }

                let hhdm_offset = state::HHDM_OFFSET.load(Ordering::Relaxed);
                
                if hhdm_offset != 0 {
                    let mut mapper = unsafe { memory::init(hhdm_offset) };
                    
                    // 1. UNLOCK CODE MEMORY
                    let app_addr = my_user_app as usize as u64;
                    memory::mark_as_user(&mut mapper, app_addr);
                    memory::mark_as_user(&mut mapper, app_addr + 4096);
                    
                    // 2. UNLOCK STACK MEMORY (CRITICAL FIX)
                    let stack_addr = unsafe { &USER_STACK as *const _ as u64 };
                    memory::mark_as_user(&mut mapper, stack_addr);
                    
                    writer::print("[DEBUG] Code & Stack Unlocked. Jumping...\n");

                    // 3. JUMP WITH STACK POINTER
                    let (user_code, user_data) = gdt::get_user_selectors();
                    // Calculate top of stack
                    let stack_top = stack_addr + 4096;
                    
                    userspace::jump_to_code(my_user_app, user_code, user_data, stack_top);
                } else {
                    writer::print("[ERROR] HHDM Offset not initialized.\n");
                }
            },
            _ => writer::print("Unknown command.\n"),
        }
    }
}

lazy_static! {
    pub static ref SHELL: Mutex<Shell> = Mutex::new(Shell::new());
}

pub fn shell_task() {
    SHELL.lock().run();
}