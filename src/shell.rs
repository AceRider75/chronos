use crate::{input, writer, fs, userspace, gdt, memory, state, pci, rtl8139, elf, compositor}; 
use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use spin::Mutex;
use lazy_static::lazy_static;
use core::sync::atomic::Ordering;

pub struct Shell {
    command_buffer: String,
    // The Shell now owns a Window!
    pub window: compositor::Window,
}

impl Shell {
    pub fn new() -> Self {
        // Create a 800x600 Black Window for the Terminal
        let mut win = compositor::Window::new(50, 50, 800, 500, 0xFF000000);
        win.print("Chronos Terminal v1.0\n");
        win.print("> ");
        
        Shell {
            command_buffer: String::new(),
            window: win,
        }
    }

    // Helper to print to THIS window
    fn print(&mut self, text: &str) {
        self.window.print(text);
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
    }
    fn execute_command(&mut self) {
        let cmd = self.command_buffer.trim();
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        if parts.is_empty() { return; }

        match parts[0] {
            "help" => {
                self.print("Chronos Shell v0.6\n");
                self.print("  ls, cat <f>    - Filesystem\n");
                self.print("  net, ip, ping  - Networking\n");
                self.print("  exec           - Internal Syscall Test\n");
                self.print("  run <file>     - ELF Application Loader\n");
                self.print("  clear, ver     - System\n");
            },

            "ls" => {
                self.print("--- Files ---\n");
                for file in fs::list_files() {
                    self.print(&format!("- {} ({} bytes)\n", file.name, file.data.len()));
                }
            },

            "exec" => {
                self.print("[EXEC] Testing internal Syscall in Ring 3...\n");
                fn user_test() -> ! {
                    unsafe { core::arch::asm!("int 0x80"); }
                    loop { core::hint::spin_loop(); }
                }
                self.spawn_user_process_at(user_test as usize as u64);
            },

            "run" => {
                if parts.len() < 2 {
                    self.print("Usage: run <filename>\n");
                } else {
                    let filename = parts[1];
                    let files = fs::list_files();
                    if let Some(file) = files.iter().find(|f| f.name.contains(filename)) {
                        let hhdm = state::HHDM_OFFSET.load(Ordering::Relaxed);
                        
                        let file_virt_ptr = file.data.as_ptr() as u64;
                        let file_phys_addr = file_virt_ptr - hhdm;
                        
                        // Page alignment math
                        let page_offset = file_virt_ptr % 4096;
                        let load_base = 0x400_000;
                        
                        self.print("[SHELL] Aligning and Mapping App...\n");
                        
                        unsafe {
                            // Map 16 pages (64KB) to be absolutely certain we cover the ELF
                            for i in 0..16 {
                                let v = load_base + (i * 4096);
                                let p = (file_phys_addr & !0xFFF) + (i * 4096);
                                memory::map_user_page(v, p);
                            }

                            // KERNEL READ TEST
                            // We test the address we are about to jump to
                            let test_ptr = (load_base + page_offset) as *const u32;
                            if *test_ptr == 0x464c457f {
                                self.print("[OK] Kernel verified ELF header at mapped address.\n");
                            }
                        }

                        // Read the Entry Point from the header
                        let raw_entry = unsafe { *(file.data.as_ptr().add(24) as *const u64) };
                        
                        // If it's a small offset (PIC), add it to our base + page_offset
                        let jump_target = if raw_entry < load_base {
                            load_base + page_offset + raw_entry
                        } else {
                            // If it's an absolute address, we assume it's already aligned
                            raw_entry
                        };
                        
                        self.print(&format!("[SHELL] Entry Offset: {:x}, Jump: {:x}\n", raw_entry, jump_target));
                        self.spawn_user_process_at(jump_target);
                    } else {
                        self.print("File not found.\n");
                    }
                }
            },

            "net" => {
                self.print("[NET] Initializing...\n");
                let devices = pci::scan_bus();
                for dev in devices {
                    if dev.vendor_id == 0x10EC && dev.device_id == 0x8139 {
                        pci::enable_bus_mastering(dev.clone());
                        let mut driver = rtl8139::Rtl8139::new(dev);
                        
                        // Send ONLY ONCE
                        driver.send_dhcp_discover();
                        
                        self.print("[NET] Waiting for DHCP Reply...\n");
                        let mut timeout = 0;
                        loop {
                            driver.sniff_packet();
                            
                            if state::get_my_ip() != [0,0,0,0] {
                                self.print("[NET] Success!\n");
                                break;
                            }
                            
                            timeout += 1;
                            // If we haven't received an IP after a while, retry once
                            if timeout == 5000 {
                                self.print("[NET] Retrying Discover...\n");
                                driver.send_dhcp_discover();
                            }

                            if timeout > 10000 {
                                self.print("[NET] Failed. No DHCP server found.\n");
                                break;
                            }
                            
                            for _ in 0..100_000 { core::hint::spin_loop(); }
                        }
                        break;
                    }
                }
            },

            "ping" => {
                let devices = pci::scan_bus();
                for dev in devices {
                    if dev.vendor_id == 0x10EC && dev.device_id == 0x8139 {
                        pci::enable_bus_mastering(dev.clone());
                        let mut driver = rtl8139::Rtl8139::new(dev);
                        self.print("[NET] Pinging Gateway 10.0.2.2...\n");
                        for i in 1..=4 {
                            driver.send_ping(i as u16);
                            for _ in 0..200 {
                                driver.sniff_packet();
                                for _ in 0..50_000 { core::hint::spin_loop(); }
                            }
                        }
                    }
                }
            },

            "clear" => { if let Some(w) = writer::WRITER.lock().as_mut() { w.clear(); w.cursor_y = 10; } },
            "ver" => { self.print("Chronos OS v0.95 (Build: Era 2)\n"); },
            "ip" => {
                let ip = state::get_my_ip();
                self.print(&format!("Local IP: {}.{}.{}.{}\n", ip[0], ip[1], ip[2], ip[3]));
            },
            "cat" => {
                if parts.len() < 2 { self.print("Usage: cat <filename>\n"); } 
                else if let Some(content) = fs::read_file(parts[1]) {
                    self.print(&content);
                    self.print("\n");
                }
            },
            _ => self.print("Unknown command. Type 'help'.\n"),
        }
    }

    /// Transitions to Ring 3 at a specific lower-half address

fn spawn_user_process_at(&self, entry_point: u64) -> ! {
        let hhdm = state::HHDM_OFFSET.load(Ordering::Relaxed);
        let k_delta = state::KERNEL_DELTA.load(Ordering::Relaxed); // NEW: Get Kernel Delta
        
        let user_stack_virt = 0x800_000;
        
        #[repr(align(4096))]
        struct Stack([u8; 4096]);
        static mut S: Stack = Stack([0; 4096]);
        
        // FIX: Use KERNEL_DELTA to get physical address of static variable
        let s_virt = unsafe { &S as *const _ as u64 };
        let s_phys = s_virt - k_delta;
        
        unsafe {
            // Map the stack
            memory::map_user_page(user_stack_virt, s_phys);
        }

        let (code, data) = gdt::get_user_selectors();
        
        // Point to the TOP of the stack (Bottom + 4096)
        userspace::jump_to_code_raw(entry_point, code, data, user_stack_virt + 4096);
    }
}

lazy_static! {
    pub static ref SHELL: Mutex<Shell> = Mutex::new(Shell::new());
}

pub fn shell_task() {
    SHELL.lock().run();
}