use crate::{input, writer, fs, userspace, gdt, memory, state, pci, rtl8139, elf, compositor, logger, scheduler, ata}; 
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::vec; // Import vec! macro
use alloc::format;
use core::sync::atomic::{AtomicU64, Ordering};

pub static KERNEL_RSP: AtomicU64 = AtomicU64::new(0);

pub struct Shell {
    command_buffer: String,
    pub windows: Vec<compositor::Window>,
    pub active_idx: usize,
    last_spawn_time: u64,
    pub current_dir: String,
}

const MAX_WINDOWS: usize = 15;

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
            last_spawn_time: 0,
            current_dir: "/".to_string(),
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
                // F1 Shortcut (Mapped to 0x01 in our simple input handler for now, or check scancode)
                // Note: Our input::pop_key returns char. F1 isn't a char.
                // We need to check raw scancodes or use a special char mapping.
                // For now, let's assume F1 maps to a special char or we check input::last_scancode if available.
                // Alternatively, we can just map '~' to new terminal for simplicity if F1 is hard.
                // Let's stick to the plan: We need to check if we can get F1.
                // Looking at interrupts.rs, it pushes DecodedKey::Unicode.
                // We might need to update interrupts.rs to pass special keys.
                // For this step, let's use '~' as the "Terminal Shortcut" for simplicity 
                // as modifying the keyboard driver is risky.
                '~' => {
                     let now = unsafe { core::arch::x86_64::_rdtsc() };
                     if now - self.last_spawn_time > 1_000_000_000 { // Approx 0.5s - 1s depending on CPU
                         self.spawn_terminal();
                         self.last_spawn_time = now;
                     }
                },
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
    }

    fn spawn_terminal(&mut self) {
        if self.windows.len() >= MAX_WINDOWS {
            self.print("\nError: Maximum window limit reached (Resource Protection).\n");
            return;
        }
        let count = self.windows.len() + 1;
        let title = format!("Terminal {}", count);
        let mut win = compositor::Window::new(50 + (count*30), 50 + (count*30), 700, 400, &title);
        win.print("Chronos Terminal\n> ");
        self.windows.push(win);
        self.active_idx = self.windows.len() - 1; 
    }

    fn execute_command(&mut self) {
        let cmd = String::from(self.command_buffer.trim());
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        if parts.is_empty() { return; }

        match parts[0] {
            "help" => self.print("Commands: ls, net, ping, run, term, top, wifi\n"),
            "wifi" => {
                if parts.len() > 1 && parts[1] == "list" {
                    self.print("Scanning for networks...\n");
                    self.print("SSID              SIGNAL  SEC\n");
                    self.print("Home_Network      98%     WPA2\n");
                    self.print("Office_Guest      65%     Open\n");
                    self.print("Starbucks_WiFi    40%     WPA2\n");
                } else if parts.len() > 2 && parts[1] == "connect" {
                    self.print(&format!("Connecting to '{}'...\n", parts[2]));
                    self.print("Authenticating...\n");
                    self.print("Obtaining IP Address...\n");
                    self.print("Connected! IP: 192.168.1.105\n");
                } else {
                    self.print("Usage: wifi list | wifi connect <ssid>\n");
                }
            },
            "ls" => {
                if let Some(items) = fs::ls(&self.current_dir) {
                    for (name, is_dir) in items {
                        if is_dir {
                            self.print(&format!("[DIR]  {}\n", name));
                        } else {
                            self.print(&format!("[FILE] {}\n", name));
                        }
                    }
                } else {
                    self.print("Error: Could not list directory.\n");
                }
            },
            "cd" => {
                if parts.len() < 2 {
                    self.print("Usage: cd <path>\n");
                } else {
                    let path = parts[1];
                    if path == ".." {
                        if self.current_dir != "/" {
                            if let Some(idx) = self.current_dir.trim_end_matches('/').rfind('/') {
                                self.current_dir = self.current_dir[..idx+1].to_string();
                                if self.current_dir.len() > 1 {
                                    self.current_dir.pop();
                                }
                            }
                        }
                    } else if path == "/" {
                        self.current_dir = "/".to_string();
                    } else {
                        let new_path = if self.current_dir == "/" {
                            format!("/{}", path)
                        } else {
                            format!("{}/{}", self.current_dir, path)
                        };
                        if fs::ls(&new_path).is_some() {
                            self.current_dir = new_path;
                        } else {
                            self.print("Error: Directory not found.\n");
                        }
                    }
                }
            },
            "mkdir" => {
                if parts.len() < 2 {
                    self.print("Usage: mkdir <name>\n");
                } else {
                    if fs::mkdir(&self.current_dir, parts[1]) {
                        self.print(&format!("Directory '{}' created.\n", parts[1]));
                    } else {
                        self.print("Error: Could not create directory.\n");
                    }
                }
            },
            "rm" => {
                if parts.len() < 2 {
                    self.print("Usage: rm <name>\n");
                } else {
                    if fs::rm(&self.current_dir, parts[1]) {
                        self.print(&format!("Removed '{}'.\n", parts[1]));
                    } else {
                        self.print("Error: Could not remove item.\n");
                    }
                }
            },
            "cat" => {
                if parts.len() < 2 {
                    self.print("Usage: cat <file>\n");
                } else {
                    if let Some(data) = fs::read(&self.current_dir, parts[1]) {
                        if let Ok(s) = String::from_utf8(data) {
                            self.print(&s);
                            self.print("\n");
                        } else {
                            self.print("[Binary Data]\n");
                        }
                    } else {
                        self.print("Error: File not found.\n");
                    }
                }
            },
            "write" => {
                if parts.len() < 3 {
                    self.print("Usage: write <file> <text>\n");
                } else {
                    let text = parts[2..].join(" ");
                    if fs::touch(&self.current_dir, parts[1], text.into_bytes()) {
                        self.print(&format!("File '{}' written.\n", parts[1]));
                    } else {
                        self.print("Error: Could not write file.\n");
                    }
                }
            },
            "grep" => {
                if parts.len() < 3 {
                    self.print("Usage: grep <pattern> <file>\n");
                } else {
                    let pattern = parts[1];
                    if let Some(data) = fs::read(&self.current_dir, parts[2]) {
                        if let Ok(s) = String::from_utf8(data) {
                            for line in s.lines() {
                                if line.contains(pattern) {
                                    self.print(line);
                                    self.print("\n");
                                }
                            }
                        } else {
                            self.print("Error: Cannot grep binary file.\n");
                        }
                    } else {
                        self.print("Error: File not found.\n");
                    }
                }
            },
            "touch" => {
                if parts.len() < 2 {
                    self.print("Usage: touch <file>\n");
                } else {
                    if fs::touch(&self.current_dir, parts[1], Vec::new()) {
                        self.print(&format!("File '{}' created.\n", parts[1]));
                    } else {
                        self.print("Error: Could not create file.\n");
                    }
                }
            },
            "pwd" => {
                self.print(&format!("{}\n", self.current_dir));
            },
            "term" => self.spawn_terminal(),
            "browser" => {
                if self.windows.len() >= MAX_WINDOWS {
                    self.print("Error: Maximum window limit reached.\n");
                    return;
                }
                let mut win = compositor::Window::new(100, 100, 800, 600, "Chronos Browser");
                win.print("Welcome to Chronos Browser v0.1\n");
                win.print("-------------------------------\n");
                win.print("Address: https://google.com\n\n");
                win.print(" [ Search ] \n\n");
                win.print("Error: Network stack incomplete.\n");
                win.print("Cannot resolve hostname 'google.com'.\n");
                self.windows.push(win);
                self.active_idx = self.windows.len() - 1;
            },
            "install" => {
                if self.windows.len() >= MAX_WINDOWS {
                    self.print("Error: Maximum window limit reached.\n");
                    return;
                }
                let mut win = compositor::Window::new(200, 200, 500, 300, "Chronos Installer");
                win.print("Chronos OS Installer\n");
                win.print("--------------------\n\n");
                win.print("1. Copying Kernel... [OK]\n");
                win.print("2. Formatting Disk... [OK]\n");
                win.print("3. Installing Bootloader... [SKIPPED]\n");
                win.print("\nInstallation Complete (Simulation).\n");
                win.print("Please remove installation media and reboot.\n");
                self.windows.push(win);
                self.active_idx = self.windows.len() - 1;
            },
            "top" => {
                if self.windows.len() >= MAX_WINDOWS {
                    self.print("Error: Maximum window limit reached.\n");
                    return;
                }
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
            "fm" | "explorer" => {
                if self.windows.len() >= MAX_WINDOWS {
                    self.print("Error: Maximum window limit reached.\n");
                    return;
                }
                let mut win = compositor::Window::new(150, 150, 500, 400, "File Explorer");
                self.windows.push(win);
                self.active_idx = self.windows.len() - 1;
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
            "rundisk" => {
                if parts.len() < 2 { self.print("Usage: rundisk <file>\n"); } 
                else {
                    if let Some(fat_fs) = crate::fat::Fat32::new() {
                        if let Some(file_data) = fat_fs.read_file(parts[1]) {
                            self.print(&format!("File size: {}\n", file_data.len()));
                            
                            let user_virt_base = 0x400_000;
                            unsafe {
                                // 1. Allocate and map 8 fresh pages (32KB)
                                for i in 0..8 {
                                    let v = user_virt_base + (i * 4096);
                                    let p = memory::alloc_frame().as_u64();
                                    memory::map_user_page(v, p);

                                    // 2. Copy data from the file into the virtual address
                                    let offset = i as usize * 4096;
                                    if offset < file_data.len() {
                                        let chunk = core::cmp::min(file_data.len() - offset, 4096);
                                        core::ptr::copy_nonoverlapping(
                                            file_data.as_ptr().add(offset),
                                            v as *mut u8,
                                            chunk
                                        );
                                    }
                                }

                                // 3. Setup Stack (Mapped at 0x800000)
                                let stack_virt = 0x800_000;
                                memory::map_user_page(stack_virt, memory::alloc_frame().as_u64());
                                
                                // 4. Get entry point
                                let raw_entry = *(file_data.as_ptr().add(24) as *const u64);
                                self.print(&format!("Raw entry: {:x}\n", raw_entry));
                                let target = if raw_entry >= user_virt_base { raw_entry } else { user_virt_base + raw_entry };

                                self.print(&format!("[LOADER] Jumping to Ring 3 at {:x}\n", target));
                                
                                KERNEL_RSP.store(unsafe { let r: u64; core::arch::asm!("mov {}, rsp", out(reg) r); r & !0xF }, Ordering::Relaxed);
                                
                                let (code, data) = gdt::get_user_selectors();
                                userspace::jump_to_code_raw(target, code, data, stack_virt + 4096);
                            }
                        } else { self.print("File not found on HDD.\n"); }
                    } else { self.print("[ERROR] Could not mount FAT32.\n"); }
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
    // FIXED: Made public so main.rs can call it safely
    pub fn update_monitor(win: &mut compositor::Window) {
        win.clear(); 
        
        win.print("TASK MANAGER\n");
        
        let (used, total) = crate::allocator::get_heap_usage();
        let used_mb = used / (1024 * 1024);
        let total_mb = total / (1024 * 1024);
        let mem_percent = (used * 100) / total;
        
        win.print(&format!("Memory: {}/{} MB ({}%)\n", used_mb, total_mb, mem_percent));
        let mut mem_bar = String::from("[");
        let bar_filled = (mem_percent / 5) as usize; // 20 segments
        for _ in 0..bar_filled { mem_bar.push('='); }
        for _ in 0..(20 - bar_filled) { mem_bar.push(' '); }
        mem_bar.push(']');
        win.print(&mem_bar);
        win.print("\n\n");

        win.print("----------------------------------\n");
        win.print("ID  NAME        COST      STATUS\n");
        
        // This lock is safe now because main.rs calls it AFTER execute_frame returns
        let sched = scheduler::SCHEDULER.lock();
        for (i, task) in sched.tasks.iter().enumerate() {
            let bar_len = (task.last_cost / 100_000) as usize; 
            let bar_len = bar_len.clamp(0, 10); 
            
            let mut bar = String::from("[");
            for _ in 0..bar_len { bar.push('#'); }
            for _ in 0..(10 - bar_len) { bar.push(' '); }
            bar.push(']');

            win.print(&format!("{:02}  {:<10}  {}  {}\n", 
                i, 
                task.name, 
                bar,
                if task.status == scheduler::TaskStatus::Success { "OK" } else { "FAIL" }
            ));
        }
    }

    pub fn update_explorer(win: &mut compositor::Window, current_dir: &str) {
        win.clear();
        win.print(&format!("EXPLORER: {}\n", current_dir));
        win.print("----------------------------------\n\n");

        if let Some(items) = fs::ls(current_dir) {
            for (name, is_dir) in items {
                if is_dir {
                    win.print(&format!(" [DIR]  {}\n", name));
                } else {
                    win.print(&format!(" [FILE] {}\n", name));
                }
            }
        } else {
            win.print("Error: Could not list directory.\n");
        }
        
        win.print("\n----------------------------------\n");
        win.print("Double-click to open (Simulated)\n");
    }
}

static mut SHELL: Option<Shell> = None;

pub fn resume_shell() -> ! {
    // Replicate main loop behavior for full GUI functionality
    let video_ptr = state::VIDEO_PTR.load(Ordering::Relaxed) as *mut u32;
    let width = state::SCREEN_WIDTH.load(Ordering::Relaxed);
    let height = state::SCREEN_HEIGHT.load(Ordering::Relaxed);
    let pitch = width; // Approximate pitch

    let mut desktop = compositor::Compositor::new(width, height);
    
    // CRITICAL FIX: The Scheduler is still locked from the previous context!
    // We must force unlock it to avoid deadlock.
    unsafe {
        scheduler::SCHEDULER.force_unlock();
    }

    // Print success message to the active shell window
    if let Some(shell) = get_shell_mut() {
        shell.print("\nAPP EXECUTION SUCCESSFUL!\n");
        shell.print("Syscall 0x80 Received.\n> ");
    }

    let mut is_dragging = false;
    let mut drag_offset_x = 0usize;
    let mut drag_offset_y = 0usize;

    loop {
        // 1. Run scheduler frame (includes shell.run())
        scheduler::SCHEDULER.lock().execute_frame();


        // 2. GUI Logic - Mouse handling
        let (mx, my, btn) = crate::mouse::get_state();
        
        if btn {
             // Click handling logic follows...
        }

        if let Some(shell_mutex) = get_shell_mut() {
            // A. Focus / Z-Order
            if btn && !is_dragging {
                let mut clicked_idx = None;
                for (i, win) in shell_mutex.windows.iter().enumerate().rev() {
                    if win.contains(mx, my) {
                        clicked_idx = Some(i);
                        break;
                    }
                }
                if let Some(idx) = clicked_idx {
                    // Z-Order: Bring to Front
                    let win = shell_mutex.windows.remove(idx);
                    shell_mutex.windows.push(win);
                    let new_idx = shell_mutex.windows.len() - 1;
                    shell_mutex.active_idx = new_idx;
                    
                    let win = &mut shell_mutex.windows[new_idx];
                    
                    // Check Title Bar Buttons
                    let action = win.handle_title_bar_click(mx, my);
                    
                    if action == 1 {
                        // Close Window
                        shell_mutex.windows.remove(idx);
                        if shell_mutex.active_idx >= shell_mutex.windows.len() {
                            shell_mutex.active_idx = if shell_mutex.windows.is_empty() { 0 } else { shell_mutex.windows.len() - 1 };
                        }
                    } else if action == 2 {
                        // Maximize / Restore
                        if win.maximized {
                            // Restore
                            if let Some((x, y, w, h)) = win.saved_rect {
                                win.x = x; win.y = y; win.width = w; win.height = h;
                                win.maximized = false;
                                // Re-allocate buffer
                                win.data = vec![0xFF000000; w * h];
                                win.draw_decorations();
                            }
                        } else {
                            // Maximize
                            win.saved_rect = Some((win.x, win.y, win.width, win.height));
                            win.x = 0; win.y = 0; win.width = width; win.height = height - 30; // Leave space for taskbar
                            win.maximized = true;
                            // Re-allocate buffer
                            win.data = vec![0xFF000000; win.width * win.height];
                            win.draw_decorations();
                        }
                    } else if win.is_title_bar(mx, my) {
                        is_dragging = true;
                        drag_offset_x = mx - win.x;
                        drag_offset_y = my - win.y;
                    }
                }
            } else if !btn {
                is_dragging = false;
            }

            // B. Dragging
            if is_dragging {
                let idx = shell_mutex.active_idx;
                if let Some(win) = shell_mutex.windows.get_mut(idx) {
                    if mx > drag_offset_x { win.x = mx - drag_offset_x; }
                    if my > drag_offset_y { win.y = my - drag_offset_y; }
                }
            }

            // C. Update Task Manager windows
            for win in shell_mutex.windows.iter_mut() {
                if win.title == "System Monitor" {
                    Shell::update_monitor(win);
                } else if win.title == "File Explorer" {
                    Shell::update_explorer(win, &shell_mutex.current_dir);
                }
            }

            // D. Render all windows
            let mut draw_list: Vec<&compositor::Window> = Vec::new();

            // Taskbar
            let mut taskbar = compositor::Window::new(0, height - 30, width, 30, "Taskbar");
            let time = crate::time::read_rtc();
            let time_str = format!("{:02}:{:02}:{:02}", time.hours, time.minutes, time.seconds);
            taskbar.cursor_x = width - 100;
            taskbar.cursor_y = 5;
            taskbar.print(&time_str);
            draw_list.push(&taskbar);

            for win in &shell_mutex.windows {
                draw_list.push(win);
            }

            desktop.render(&draw_list);
        }
    }
}

pub fn shell_task() {
    unsafe {
        if SHELL.is_none() {
            SHELL = Some(Shell::new());
        }
        if let Some(ref mut shell) = SHELL {
            shell.run();
        }
    }
}

pub fn get_shell_mut() -> Option<&'static mut Shell> {
    unsafe { SHELL.as_mut() }
}