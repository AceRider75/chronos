use crate::{input, writer, fs, userspace, gdt, memory, state, pci, rtl8139, elf, compositor, logger}; 
use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use spin::Mutex;
use lazy_static::lazy_static;
use core::sync::atomic::Ordering;

pub struct Shell {
    command_buffer: String,
    pub window: compositor::Window,
}

impl Shell {
    pub fn new() -> Self {
        // Updated Constructor with Title string
        let mut win = compositor::Window::new(50, 50, 800, 500, "Terminal");
        
        win.print("Chronos Terminal v1.0\n");
        win.print("> ");
        
        Shell {
            command_buffer: String::new(),
            window: win,
        }
    }

    // Helper to print to the window
    fn print(&mut self, text: &str) {
        self.window.print(text);
    }

    // --- MAIN TASK FUNCTION ---
    pub fn run(&mut self) {
        // 1. Process all pending Keyboard Input
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

        // 2. Process all pending Kernel Logs (Network, Drivers, etc)
        // This ensures driver output appears INSIDE the window
        let logs = logger::drain();
        for msg in logs {
            self.print(&msg);
        }
    }

    fn execute_command(&mut self) {
        // FIX: Use String::from instead of to_owned()
        let cmd = String::from(self.command_buffer.trim());
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        
        if parts.is_empty() { return; }

        match parts[0] {
            "help" => {
                self.print("Chronos Shell v0.6\n");
                self.print("  ls, cat <f>    - Filesystem\n");
                self.print("  net, ip, ping  - Networking\n");
                self.print("  exec           - Internal Syscall Test\n");
                self.print("  run <file>     - ELF Loader\n");
                self.print("  clear          - Reset terminal\n");
            },
            "ls" => {
                for file in fs::list_files() {
                    let msg = format!("- {} ({} bytes)\n", file.name, file.data.len());
                    self.print(&msg);
                }
            },
            "cat" => {
                if parts.len() < 2 { self.print("Usage: cat <filename>\n"); } 
                else if let Some(content) = fs::read_file(parts[1]) {
                    self.print(&content);
                    self.print("\n");
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
                self.window.clear(); 
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