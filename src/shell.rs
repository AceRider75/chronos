use crate::{input, writer, fs, userspace, gdt, memory, state, pci, rtl8139, elf, compositor, logger}; 
use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use spin::Mutex;
use lazy_static::lazy_static;
use core::sync::atomic::Ordering;

pub struct Shell {
    command_buffer: String,
    pub windows: Vec<compositor::Window>, // Multiple windows!
    pub active_idx: usize,                // Which one gets keyboard input?
}

impl Shell {
    pub fn new() -> Self {
        // Create the first terminal
        let mut win = compositor::Window::new(50, 50, 700, 400, "Terminal 1");
        win.print("Chronos Terminal v1.0\n> ");
        
        let mut windows = Vec::new();
        windows.push(win);

        Shell {
            command_buffer: String::new(),
            windows,
            active_idx: 0,
        }
    }

    // Helper to print to the ACTIVE window
    fn print(&mut self, text: &str) {
        if let Some(win) = self.windows.get_mut(self.active_idx) {
            win.print(text);
        }
    }

    pub fn run(&mut self) {
        while let Some(c) = input::pop_key() {
            match c {
                '\n' => {
                    self.print("\n");
                    self.execute_command();
                    self.command_buffer.clear();
                    self.print("> "); 
                }
                '\x08' => {
                    if !self.command_buffer.is_empty() {
                        self.command_buffer.pop();
                        self.print("\x08"); 
                    }
                }
                _ => {
                    self.command_buffer.push(c);
                    let mut s = String::new();
                    s.push(c);
                    self.print(&s);
                }
            }
        }

        // Logs go to ALL windows? No, just the active one for now.
        let logs = logger::drain();
        for msg in logs {
            self.print(&msg);
        }
    }

    fn execute_command(&mut self) {
        let cmd = String::from(self.command_buffer.trim());
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        if parts.is_empty() { return; }

        match parts[0] {
            "help" => {
                self.print("Commands: ls, net, ping, run <f>, term\n");
            },
            
            // NEW COMMAND: Spawn a new terminal
            "term" => {
                let count = self.windows.len() + 1;
                let title = format!("Terminal {}", count);
                // Offset position slightly so they stack nicely
                let x = 50 + (count * 30);
                let y = 50 + (count * 30);
                
                let mut win = compositor::Window::new(x, y, 700, 400, &title);
                win.print("New Terminal Instance\n> ");
                
                self.windows.push(win);
                // Switch focus to new window
                self.active_idx = self.windows.len() - 1; 
            },
            "ls" => {
                for file in fs::list_files() {
                    let msg = format!("- {} ({} bytes)\n", file.name, file.data.len());
                    self.print(&msg);
                }
            },
            "touch" => {
                if parts.len() < 2 { self.print("Usage: touch <filename>\n"); } 
                else {
                    fs::create_file(parts[1]);
                    self.print("File created.\n");
                }
            },

            "rm" => {
                if parts.len() < 2 { self.print("Usage: rm <filename>\n"); } 
                else {
                    fs::delete_file(parts[1]);
                    self.print("File deleted.\n");
                }
            },

            "write" => {
                // Usage: write filename "text goes here"
                if parts.len() < 3 { 
                    self.print("Usage: write <file> <word>\n"); 
                } else {
                    let filename = parts[1];
                    // Join the rest of the parts as the content
                    // (Simple impl: just writes the first word for now, or loop to join)
                    let content = parts[2]; 
                    
                    if fs::append_file(filename, content.as_bytes()) {
                        // Add a newline for neatness
                        fs::append_file(filename, b"\n");
                        self.print("Data written.\n");
                    } else {
                        self.print("File not found.\n");
                    }
                }
            },
            
            // Update 'cat' to handle Vec<u8> properly
            "cat" => {
                if parts.len() < 2 { self.print("Usage: cat <filename>\n"); } 
                else if let Some(data) = fs::read_file(parts[1]) {
                    // Try convert to string
                    if let Ok(s) = alloc::string::String::from_utf8(data) {
                        self.print(&s);
                        self.print("\n");
                    } else {
                        self.print("[Binary Data]\n");
                    }
                } else { self.print("File not found.\n"); }
            },
            "ip" => {
                let ip = state::get_my_ip();
                let msg = format!("IP: {}.{}.{}.{}\n", ip[0], ip[1], ip[2], ip[3]);
                self.print(&msg);
            },
            "net" => {
                self.print("[NET] Initializing...\n");
                let devices = pci::scan_bus();
                for dev in devices {
                    if dev.vendor_id == 0x10EC && dev.device_id == 0x8139 {
                        pci::enable_bus_mastering(dev.clone());
                        let mut driver = rtl8139::Rtl8139::new(dev);
                        driver.send_dhcp_discover();
                        self.print("[NET] DHCP Sent. Check logs for reply.\n");
                        break;
                    }
                }
            },
            "ping" => {
                self.print("[NET] Pinging Gateway...\n");
                let devices = pci::scan_bus();
                for dev in devices {
                    if dev.vendor_id == 0x10EC && dev.device_id == 0x8139 {
                        pci::enable_bus_mastering(dev.clone());
                        let mut driver = rtl8139::Rtl8139::new(dev);
                        for i in 1..=4 {
                            driver.send_ping(i as u16);
                            // Brief wait to allow hardware to send
                            for _ in 0..200 {
                                driver.sniff_packet();
                                for _ in 0..50_000 { core::hint::spin_loop(); }
                            }
                        }
                    }
                }
            },
            "clear" => { 
                self.windows.clear(); 
                self.print("> ");
            },
            "run" => {
                if parts.len() < 2 {
                    self.print("Usage: run <filename>\n");
                } else {
                    let filename = parts[1];
                    let files = fs::list_files();
                    if let Some(file) = files.iter().find(|f| f.name.contains(filename)) {
                        let hhdm = state::HHDM_OFFSET.load(Ordering::Relaxed);
                        let file_phys = (file.data.as_ptr() as u64) - hhdm;
                        let page_offset = (file.data.as_ptr() as u64) % 4096;
                        let load_base = 0x400_000;
                        
                        self.print("[SHELL] Mapping ELF...\n");
                        unsafe {
                            for i in 0..16 {
                                let v = load_base + (i * 4096);
                                let p = (file_phys & !0xFFF) + (i * 4096);
                                memory::map_user_page(v, p);
                            }
                        }
                        
                        let raw_entry = unsafe { *(file.data.as_ptr().add(24) as *const u64) };
                        let jump_target = if raw_entry >= load_base { raw_entry } else { load_base + page_offset + raw_entry };
                        
                        let msg = format!("[SHELL] Jumping to {:x}\n", jump_target);
                        self.print(&msg);
                        self.spawn_user_process_at(jump_target);
                    } else {
                        self.print("File not found.\n");
                    }
                }
            },
            "exec" => {
                self.print("[EXEC] Testing internal Syscall...\n");
                fn user_test() -> ! {
                    unsafe { core::arch::asm!("int 0x80"); }
                    loop { core::hint::spin_loop(); }
                }
                self.spawn_user_process_at(user_test as usize as u64);
            },
            _ => self.print("Unknown command.\n"),
        }
    }

    fn spawn_user_process_at(&self, entry_point: u64) -> ! {
        let hhdm = state::HHDM_OFFSET.load(Ordering::Relaxed);
        let user_stack_virt = 0x800_000;
        
        #[repr(align(4096))]
        struct Stack([u8; 4096]);
        static mut S: Stack = Stack([0; 4096]);
        
        let k_delta = state::KERNEL_DELTA.load(Ordering::Relaxed);
        let s_phys = (unsafe { &S as *const _ as u64 }) - k_delta;
        
        unsafe {
            memory::map_user_page(user_stack_virt, s_phys);
        }

        let (code, data) = gdt::get_user_selectors();
        userspace::jump_to_code_raw(entry_point, code, data, user_stack_virt + 4096);
    }
}

lazy_static! {
    pub static ref SHELL: Mutex<Shell> = Mutex::new(Shell::new());
}

pub fn shell_task() {
    SHELL.lock().run();
}