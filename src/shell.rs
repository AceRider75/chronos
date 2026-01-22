use crate::{input, writer, fs, userspace, gdt, memory, state, pci, rtl8139, elf, compositor, logger, scheduler, ata}; 
use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use spin::Mutex;
use lazy_static::lazy_static;
use core::sync::atomic::Ordering;

pub struct Shell {
    command_buffer: String,
    pub windows: Vec<compositor::Window>,
    pub active_idx: usize,
}

impl Shell {
    pub fn new() -> Self {
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

    fn print(&mut self, text: &str) {
        if let Some(win) = self.windows.get_mut(self.active_idx) {
            win.print(text);
        }
    }

    pub fn run(&mut self) {
        // 1. Input
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

        // 2. Logs
        let logs = logger::drain();
        for msg in logs {
            self.print(&msg);
        }

        // REMOVED: The update_monitor loop. 
        // We moved this logic to main.rs to avoid Deadlock!
    }

    // FIXED: Made public so main.rs can call it safely
    pub fn update_monitor(win: &mut compositor::Window) {
        win.clear(); 
        
        win.print("TASK MANAGER\n");
        win.print("----------------------------------\n");
        win.print("ID  NAME        COST      STATUS\n");
        
        // This lock is safe now because main.rs calls it AFTER execute_frame returns
        let sched = scheduler::SCHEDULER.lock();
        for (i, task) in sched.tasks.iter().enumerate() {
            let bar_len = (task.last_cost / 100_000) as usize; 
            let bar_len = bar_len.clamp(0, 10); 
            
            let mut bar = String::from("[");
            for _ in 0..bar_len { bar.push('#'); }
            for _ in 0..(10 - bar_len) { bar.push('.'); }
            bar.push(']');

            let status = match task.status {
                scheduler::TaskStatus::Success => "OK",
                scheduler::TaskStatus::Failure => "FAIL",
                _ => "..",
            };

            let safe_name = if task.name.len() > 8 { &task.name[0..8] } else { &task.name };

            let line = format!("{:02}  {:<8}  {:<8} {}\n", 
                i, safe_name, task.last_cost, status);
            
            win.print(&line);
            win.print("    "); 
            win.print(&bar);
            win.print("\n");
        }
    }

    fn execute_command(&mut self) {
        let cmd = String::from(self.command_buffer.trim());
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        if parts.is_empty() { return; }

        match parts[0] {
            "help" => self.print("Commands: ls, net, ping, run, term, top\n"),
            "ls" => {
                for file in fs::list_files() {
                    self.print(&format!("- {} ({} bytes)\n", file.name, file.data.len()));
                }
            },
            "term" => {
                let count = self.windows.len() + 1;
                let title = format!("Terminal {}", count);
                let mut win = compositor::Window::new(50 + (count*30), 50 + (count*30), 700, 400, &title);
                win.print("New Terminal Instance\n> ");
                self.windows.push(win);
                self.active_idx = self.windows.len() - 1; 
            },
            "top" => {
                let mut win = compositor::Window::new(300, 100, 400, 500, "System Monitor");
                self.windows.push(win);
                self.active_idx = self.windows.len() - 1;
            },
            "net" => {
                self.print("Initializing Network...\n");
                let devices = pci::scan_bus();
                for dev in devices {
                    if dev.vendor_id == 0x10EC && dev.device_id == 0x8139 {
                        pci::enable_bus_mastering(dev.clone());
                        let mut driver = rtl8139::Rtl8139::new(dev);
                        driver.send_dhcp_discover();
                        loop {
                            driver.sniff_packet();
                            if state::get_my_ip() != [0,0,0,0] { self.print("Success!\n"); break; }
                            for _ in 0..50_000 { core::hint::spin_loop(); }
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
            "run" => {
                if parts.len() < 2 { self.print("Usage: run <filename>\n"); } else {
                    if let Some(file) = fs::list_files().iter().find(|f| f.name.contains(parts[1])) {
                        let hhdm = state::HHDM_OFFSET.load(Ordering::Relaxed);
                        let file_phys = (file.data.as_ptr() as u64) - hhdm;
                        let load_base = 0x400_000;
                        unsafe {
                            for i in 0..16 {
                                memory::map_user_page(load_base + (i*4096), (file_phys & !0xFFF) + (i*4096));
                            }
                        }
                        let raw = unsafe { *(file.data.as_ptr().add(24) as *const u64) };
                        let target = if raw >= load_base { raw } else { load_base + (file.data.as_ptr() as u64 % 4096) + raw };
                        self.print(&format!("Jumping to {:x}\n", target));
                        self.spawn_user_process_at(target);
                    } else { self.print("File not found.\n"); }
                }
            },
            "disk" => {
                let drive = ata::AtaDrive::new(true); // Master Drive
                if drive.identify() {
                    self.print("[DISK] ATA Master Drive Detected.\n");
                    
                    if parts.len() > 2 && parts[1] == "write" {
                        // FIX: Combine all parts starting from index 2
                        let content = parts[2..].join(" "); 
                        let data = content.as_bytes();
                        
                        // Prepare 512-byte buffer
                        let mut sector = [0u8; 512];
                        for (i, &b) in data.iter().enumerate() {
                            if i < 512 { sector[i] = b; }
                        }
                        
                        self.print(&format!("[DISK] Writing '{}' to Sector 0...\n", content));
                        drive.write_sectors(0, &sector);
                        self.print("[DISK] Write complete.\n");
                    } 
                    else if parts.len() > 1 && parts[1] == "read" {
                        self.print("[DISK] Reading Sector 0...\n");
                        let data = drive.read_sectors(0, 1);
                        
                        self.print("Data: ");
                        for i in 0..512 { // Scan whole sector
                            let c = data[i];
                            if c == 0 { break; } // Stop at null terminator
                            if c >= 32 && c <= 126 {
                                let mut s = String::new();
                                s.push(c as char);
                                self.print(&s);
                            } else {
                                self.print(".");
                            }
                        }
                        self.print("\n");
                    } else {
                        self.print("Usage: disk read | disk write <text>\n");
                    }
                } else {
                    self.print("[DISK] No drive found.\n");
                }
            },  
            "lsdisk" => {
                writer::print("[SHELL] Mounting HDD (FAT32)...\n");
                if let Some(fs) = crate::fat::Fat32::new() {
                    fs.list_root();
                } else {
                    writer::print("[ERROR] Could not mount FAT32.\n");
                }
            },  
            "catdisk" => {
                if parts.len() < 2 {
                    writer::print("Usage: catdisk <filename>\n");
                } else {
                    let filename = parts[1];
                    writer::print(&format!("[DISK] Reading '{}' from HDD...\n", filename));
                    
                    if let Some(fs) = crate::fat::Fat32::new() {
                        if let Some(data) = fs.read_file(filename) {
                            // Try to print as string
                            if let Ok(s) = alloc::string::String::from_utf8(data) {
                                writer::print("--- FILE START ---\n");
                                writer::print(&s);
                                writer::print("\n--- FILE END ---\n");
                            } else {
                                writer::print("[Binary Data]\n");
                            }
                        } else {
                            writer::print("File not found on disk.\n");
                        }
                    } else {
                        writer::print("[ERROR] Mount failed.\n");
                    }
                }
            },                          
            "ip" => {
                let ip = state::get_my_ip();
                self.print(&format!("IP: {}.{}.{}.{}\n", ip[0], ip[1], ip[2], ip[3]));
            },
            "clear" => { self.windows.clear(); self.print("> "); },
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
        
        unsafe { memory::map_user_page(user_stack_virt, s_phys); }
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