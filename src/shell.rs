use crate::{input, writer, fs, userspace, gdt, memory, state, pci, rtl8139, elf, compositor, logger, scheduler, ata}; 
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::vec; // Import vec! macro
use alloc::format;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;
use lazy_static::lazy_static;

pub static KERNEL_RSP: AtomicU64 = AtomicU64::new(0);

pub struct Shell {
    command_buffer: String,
    pub windows: Vec<compositor::Window>,
    pub active_idx: usize,
    last_spawn_time: u64,
    pub current_dir: String,
    pub history: Vec<String>,
    pub history_idx: usize,
    pub clipboard: String,
    pub nano_status: String,
    pub insertion_point: usize,
    pub prompt_start_idx: usize,
    pub prompt_start_y: usize,
}

const MAX_WINDOWS: usize = 15;

impl Shell {
    pub fn new() -> Self {
        let mut win = compositor::Window::new(50, 50, 700, 400, "Terminal 1");
        
        let mut windows = Vec::new();
        windows.push(win);

        let mut s = Shell {
            command_buffer: String::new(),
            windows,
            active_idx: 0,
            last_spawn_time: 0,
            current_dir: "/".to_string(),
            history: Vec::new(),
            history_idx: 0,
            clipboard: String::new(),
            nano_status: String::new(),
            insertion_point: 0,
            prompt_start_idx: 0,
            prompt_start_y: compositor::TITLE_HEIGHT + 4,
        };
        
        // Correct initialization for the first window
        if let Some(win) = s.windows.get_mut(0) {
            win.print("Chronos Terminal v1.0\n");
            s.prompt_start_idx = win.text_buffer.chars().count();
            s.prompt_start_y = win.cursor_y;
            win.print("> ");
        }

        s.load_history();
        s
    }

    fn load_history(&mut self) {
        if let Some(data) = fs::read("/", ".bash_history") {
            if let Ok(s) = String::from_utf8(data) {
                self.history = s.lines().map(|l| l.to_string()).collect();
                self.history_idx = self.history.len();
            }
        }
    }

    fn save_history(&self) {
        let mut data = String::new();
        for h in &self.history {
            data.push_str(h);
            data.push('\n');
        }
        fs::touch("/", ".bash_history", data.into_bytes());
        fs::save_to_disk();
    }

    fn print(&mut self, text: &str) {
        if let Some(win) = self.windows.get_mut(self.active_idx) {
            win.print(text);
        }
    }

    pub fn run(&mut self) {
        // 1. Process Input
        // LIMIT THROUGHPUT: Only process up to 10 keys per tick to avoid blowing the budget
        // and entering the "Penalty Box". This keeps the UI responsive even if user types fast.
        let mut processed_count = 0;
        
        while let Some(c) = input::pop_key() {
            if processed_count >= 10 {
                break;
            }
            processed_count += 1;
            let active_idx = self.active_idx;
            if let Some(win) = self.windows.get_mut(active_idx) {
                if win.title.starts_with("Nano - ") {
                    // NANO INPUT HANDLING
                    match c {
                        '\x08' => { // Backspace
                            if !win.text_buffer.is_empty() {
                                win.text_buffer.pop();
                                let text = win.text_buffer.clone();
                                win.clear();
                                win.print(&text);
                            }
                        }
                        '\x13' | '\x0F' => { // Ctrl+S or Ctrl+O (Save)
                            let filename = win.title.trim_start_matches("Nano - ").to_string();
                            let content = win.text_buffer.clone();
                            let len = content.len();
                            fs::touch(&self.current_dir, &filename, content.into_bytes());
                            fs::save_to_disk();
                            self.nano_status = format!("[ Saved {} bytes ]", len);
                        }
                        '\x18' => { // Ctrl+X (Exit)
                            self.windows.remove(active_idx);
                            if self.active_idx >= self.windows.len() {
                                self.active_idx = if self.windows.is_empty() { 0 } else { self.windows.len() - 1 };
                            }
                            return; // Exit the run() call for this frame
                        }
                        '\x0B' => { // Ctrl+K (Cut)
                            self.clipboard = win.text_buffer.clone();
                            win.text_buffer.clear();
                            win.clear();
                            self.nano_status = format!("[ Cut {} characters ]", self.clipboard.len());
                        }
                        '\x15' => { // Ctrl+U (Uncut/Paste)
                            let clip = self.clipboard.clone();
                            win.print(&clip);
                            self.nano_status = format!("[ Uncut {} characters ]", clip.len());
                        }
                        '\x03' => { // Ctrl+C (Cur Pos)
                            self.nano_status = format!("[ Line {}, Col {} ]", win.cursor_y / 18, win.cursor_x / 9);
                        }
                        '\x07' => { // Ctrl+G (Get Help)
                            self.nano_status = "[ Shortcuts: ^O Save, ^X Exit, ^K Cut, ^U Paste, ^R Read ]".to_string();
                        }
                        '\x12' => { // Ctrl+R (Read File)
                            // For now, let's just simulate reading a file named 'import.txt'
                            if let Some(data) = fs::read(&self.current_dir, "import.txt") {
                                if let Ok(s) = String::from_utf8(data) {
                                    win.print(&s);
                                    self.nano_status = "[ Read import.txt ]".to_string();
                                }
                            } else {
                                self.nano_status = "[ Error: import.txt not found ]".to_string();
                            }
                        }
                        _ => {
                            let mut s = String::new();
                            s.push(c);
                            win.print(&s);
                        }
                    }
                    continue; // Skip terminal handling
                }
            }

            match c {
                '\n' => {
                    self.print("\n");
                    self.execute_command();
                    self.command_buffer.clear();
                    self.insertion_point = 0;
                    if let Some(win) = self.windows.get_mut(self.active_idx) {
                        self.prompt_start_idx = win.text_buffer.chars().count();
                        self.prompt_start_y = win.cursor_y;
                    }
                    self.print("> "); 
                }
                '\x08' => {
                    if self.insertion_point > 0 {
                        self.insertion_point -= 1;
                        self.command_buffer.remove(self.insertion_point);
                        self.redraw_command_line();
                    }
                }
                '\u{E000}' => { // Up Arrow
                    if !self.history.is_empty() && self.history_idx > 0 {
                        self.history_idx -= 1;
                        self.command_buffer = self.history[self.history_idx].clone();
                        self.insertion_point = self.command_buffer.len();
                        self.redraw_command_line();
                    }
                }
                '\u{E001}' => { // Down Arrow
                    if self.history_idx < self.history.len() {
                        self.history_idx += 1;
                        if self.history_idx < self.history.len() {
                            self.command_buffer = self.history[self.history_idx].clone();
                        } else {
                            self.command_buffer.clear();
                        }
                        self.insertion_point = self.command_buffer.len();
                        self.redraw_command_line();
                    }
                }
                '\u{E002}' => { // Left Arrow
                    if self.insertion_point > 0 {
                        self.insertion_point -= 1;
                        self.redraw_command_line();
                    }
                }
                '\u{E003}' => { // Right Arrow
                    if self.insertion_point < self.command_buffer.len() {
                        self.insertion_point += 1;
                        self.redraw_command_line();
                    }
                }
                '\u{E006}' => { // Delete Key
                    if self.insertion_point < self.command_buffer.len() {
                        self.command_buffer.remove(self.insertion_point);
                        self.redraw_command_line();
                    }
                }
                '\u{E004}' => { // Copy (Ctrl+Shift+C)
                    if let Some(win) = self.windows.get_mut(self.active_idx) {
                        self.clipboard = win.get_selected_text();
                        win.clear_selection();
                    }
                }
                '\u{E005}' => { // Paste (Ctrl+Shift+V)
                    let clip = self.clipboard.clone();
                    for c in clip.chars() {
                        if c == '\n' || c == '\r' { continue; }
                        self.command_buffer.insert(self.insertion_point, c);
                        self.insertion_point += 1;
                    }
                    self.redraw_command_line();
                }
                '~' => {
                     let now = unsafe { core::arch::x86_64::_rdtsc() };
                     if now - self.last_spawn_time > 1_000_000_000 { 
                         self.spawn_terminal();
                     }
                },
                _ => {
                    self.command_buffer.insert(self.insertion_point, c);
                    self.insertion_point += 1;
                    self.redraw_command_line();
                }
            }
        }

        // 2. Yield if nothing happened


        // 3. Logs
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
        if !cmd.is_empty() {
            if self.history.last() != Some(&cmd.to_string()) {
                self.history.push(cmd.to_string());
                self.save_history();
            }
            self.history_idx = self.history.len();
        }

        let parts: Vec<&str> = cmd.split_whitespace().collect();
        if parts.is_empty() { return; }

        match parts[0] {
            "help" => self.print("Commands: ls, net, ping, run, term, top, wifi\n"),
            "wifi" => {
                if parts.len() > 1 && parts[1] == "list" {
                    self.print("Scanning for networks...\n");
                    // Simulate a delay/scan
                    for _ in 0..500000 { core::hint::spin_loop(); } 
                    self.print("SSID              SIGNAL  SEC   CH\n");
                    self.print("----------------------------------\n");
                    self.print("Home_5G           98%     WPA3  36\n");
                    self.print("Chronos_Internal  82%     WPA2  1\n");
                    self.print("Office_Guest      65%     Open  11\n");
                    self.print("Starbucks_WiFi    40%     WPA2  6\n");
                    self.print("Neighbour_4       22%     WPA2  11\n");
                } else if parts.len() > 2 && parts[1] == "connect" {
                    self.print(&format!("Connecting to '{}'...\n", parts[2]));
                    self.print("  [.] Handshake\n");
                    for _ in 0..300000 { core::hint::spin_loop(); }
                    self.print("  [..] Authenticating\n");
                    for _ in 0..300000 { core::hint::spin_loop(); }
                    self.print("  [...] Obtaining IP Address\n");
                    for _ in 0..300000 { core::hint::spin_loop(); }
                    self.print("Connected! IP: 192.168.1.105 (DNS: 1.1.1.1)\n");
                } else {
                    self.print("Usage: wifi list | wifi connect <ssid>\n");
                }
            },
            "browser" => {
                if self.windows.len() >= 10 { // Use hardcoded limit for now
                    self.print("Error: Maximum window limit reached.\n");
                    return;
                }
                let mut win = compositor::Window::new(150, 150, 600, 450, "Web Browser - Google");
                win.clear();
                win.print("Welcome to Chronos Browser\n");
                win.print("--------------------------\n");
                win.print("Type 'goto <url>' to browse.\n");
                self.windows.push(win);
                self.active_idx = self.windows.len() - 1;
                self.print("Launched Web Browser.\n");
            },
            "install" => {
                self.print("Initializing Chronos Setup...\n");
                self.print("  [####                ] 20% - Copying Core Files\n");
                for _ in 0..1000000 { core::hint::spin_loop(); }
                self.print("  [########            ] 40% - Configuring Drivers\n");
                for _ in 0..1000000 { core::hint::spin_loop(); }
                self.print("  [############        ] 60% - Setting up User Environment\n");
                for _ in 0..1000000 { core::hint::spin_loop(); }
                self.print("  [################    ] 80% - Finalizing System\n");
                for _ in 0..1000000 { core::hint::spin_loop(); }
                self.print("  [####################] 100% - Done!\n");
                self.print("System installed successfully. Please reboot.\n");
            },
            "goto" => {
                if parts.len() < 2 { self.print("Usage: goto <url>\n"); }
                else {
                    let url = parts[1];
                    self.print(&format!("Navigating to {}...\n", url));
                    // Find the browser window
                    for win in self.windows.iter_mut() {
                        if win.title == "Web Browser - Google" {
                            win.clear();
                            win.print(&format!("ADDRESS: {}\n", url));
                            win.print("--------------------------\n\n");
                            win.print("Status: Resolving host...\n");
                            for _ in 0..200000 { core::hint::spin_loop(); }
                            win.print("Status: Connecting...\n");
                            for _ in 0..200000 { core::hint::spin_loop(); }
                            win.print("Status: Fetching HTML...\n");
                            for _ in 0..200000 { core::hint::spin_loop(); }
                            win.print("\n[ CONTENT ]\n");
                            win.print("Welcome to the web! This is a simulated\n");
                            win.print("HTML page rendered in text mode.\n");
                            win.print("\nNavigation complete.\n");
                        }
                    }
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
                        fs::save_to_disk();
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
                        fs::save_to_disk();
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
                        fs::save_to_disk();
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
                        fs::save_to_disk();
                    } else {
                        self.print("Error: Could not create file.\n");
                    }
                }
            },
            "pwd" => {
                self.print(&format!("{}\n", self.current_dir));
            },
            "cp" => {
                if parts.len() < 3 {
                    self.print("Usage: cp <src> <dest>\n");
                } else {
                    if fs::copy_node(&self.current_dir, parts[1], &self.current_dir, parts[2]) {
                        self.print(&format!("Copied '{}' to '{}'.\n", parts[1], parts[2]));
                        fs::save_to_disk();
                    } else {
                        self.print("Error: Could not copy.\n");
                    }
                }
            },
            "mv" => {
                if parts.len() < 3 {
                    self.print("Usage: mv <src> <dest>\n");
                } else {
                    if fs::move_node(&self.current_dir, parts[1], &self.current_dir, parts[2]) {
                        self.print(&format!("Moved '{}' to '{}'.\n", parts[1], parts[2]));
                        fs::save_to_disk();
                    } else {
                        self.print("Error: Could not move.\n");
                    }
                }
            },
            "find" => {
                if parts.len() < 2 {
                    self.print("Usage: find <pattern>\n");
                } else {
                    let pattern = parts[1];
                    fs::walk_tree("/", |path, node| {
                        if node.name().contains(pattern) {
                            self.print(&format!("{}\n", path));
                        }
                    });
                }
            },
            "du" => {
                let mut total_size = 0;
                fs::walk_tree(&self.current_dir, |_, node| {
                    if let fs::Node::File { data, .. } = node {
                        total_size += data.len();
                    }
                });
                self.print(&format!("Total size: {} bytes\n", total_size));
            },
            "stat" => {
                if parts.len() < 2 {
                    self.print("Usage: stat <file>\n");
                } else {
                    if let Some(info) = fs::get_node_info(&self.current_dir, parts[1]) {
                        self.print(&format!("Name: {}\n", info.name));
                        self.print(&format!("Type: {}\n", if info.is_dir { "Directory" } else { "File" }));
                        if !info.is_dir {
                            self.print(&format!("Size: {} bytes\n", info.size));
                        } else {
                            self.print(&format!("Children: {}\n", info.child_count));
                        }
                    } else {
                        self.print("Error: Not found.\n");
                    }
                }
            },
            "head" => {
                if parts.len() < 2 {
                    self.print("Usage: head <file> [-n lines]\n");
                } else {
                    let mut n = 10;
                    if parts.len() > 3 && parts[2] == "-n" {
                        n = parts[3].parse().unwrap_or(10);
                    }
                    if let Some(data) = fs::read(&self.current_dir, parts[1]) {
                        if let Ok(s) = String::from_utf8(data) {
                            for line in s.lines().take(n) {
                                self.print(line);
                                self.print("\n");
                            }
                        }
                    } else {
                        self.print("Error: File not found.\n");
                    }
                }
            },
            "tail" => {
                if parts.len() < 2 {
                    self.print("Usage: tail <file> [-n lines]\n");
                } else {
                    let mut n = 10;
                    if parts.len() > 3 && parts[2] == "-n" {
                        n = parts[3].parse().unwrap_or(10);
                    }
                    if let Some(data) = fs::read(&self.current_dir, parts[1]) {
                        if let Ok(s) = String::from_utf8(data) {
                            let lines: Vec<&str> = s.lines().collect();
                            let start = if lines.len() > n { lines.len() - n } else { 0 };
                            for line in &lines[start..] {
                                self.print(line);
                                self.print("\n");
                            }
                        }
                    } else {
                        self.print("Error: File not found.\n");
                    }
                }
            },
            "wc" => {
                if parts.len() < 2 {
                    self.print("Usage: wc <file>\n");
                } else {
                    if let Some(data) = fs::read(&self.current_dir, parts[1]) {
                        let bytes = data.len();
                        if let Ok(s) = String::from_utf8(data) {
                            let lines = s.lines().count();
                            let words = s.split_whitespace().count();
                            self.print(&format!("{} {} {} {}\n", lines, words, bytes, parts[1]));
                        } else {
                            self.print(&format!("- - {} {}\n", bytes, parts[1]));
                        }
                    } else {
                        self.print("Error: File not found.\n");
                    }
                }
            },
            "echo" => {
                let mut redirect_idx = None;
                let mut append = false;
                for (i, part) in parts.iter().enumerate() {
                    if *part == ">" {
                        redirect_idx = Some(i);
                        break;
                    } else if *part == ">>" {
                        redirect_idx = Some(i);
                        append = true;
                        break;
                    }
                }

                if let Some(idx) = redirect_idx {
                    if idx + 1 < parts.len() {
                        let text = parts[1..idx].join(" ");
                        let filename = parts[idx+1];
                        let mut final_data = if append {
                            fs::read(&self.current_dir, filename).unwrap_or_default()
                        } else {
                            Vec::new()
                        };
                        final_data.extend_from_slice(text.as_bytes());
                        final_data.push(b'\n');
                        
                        if fs::touch(&self.current_dir, filename, final_data) {
                            fs::save_to_disk();
                        } else {
                            self.print("Error: Could not write to file.\n");
                        }
                    } else {
                        self.print("Usage: echo <text> [>|>> file]\n");
                    }
                } else {
                    let text = parts[1..].join(" ");
                    self.print(&text);
                    self.print("\n");
                }
            },

            "term" => self.spawn_terminal(),
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
            "nano" => {
                if parts.len() < 2 {
                    self.print("Usage: nano <file>\n");
                } else {
                    if self.windows.len() >= MAX_WINDOWS {
                        self.print("Error: Maximum window limit reached.\n");
                        return;
                    }
                    let filename = parts[1].to_string();
                    let content = fs::read(&self.current_dir, &filename)
                        .and_then(|d| String::from_utf8(d).ok())
                        .unwrap_or_default();
                    
                    let mut win = compositor::Window::new(100, 100, 600, 450, &format!("Nano - {}", filename));
                    win.print(&content);
                    self.windows.push(win);
                    self.active_idx = self.windows.len() - 1;
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
        win.print("----------------------------------\n");
        
        let (used, total) = crate::allocator::get_heap_usage();
        win.print(&format!("Memory: {} / {} KB\n\n", used/1024, total/1024));

        // Copy task data while interrupts are disabled, then print after
        let task_data: alloc::vec::Vec<(usize, alloc::string::String, &'static str, u64)> = 
            x86_64::instructions::interrupts::without_interrupts(|| {
                let sched = scheduler::SCHEDULER.lock();
                sched.tasks.iter().enumerate().map(|(i, task)| {
                    let status = match task.status {
                        scheduler::TaskStatus::Waiting => "WAIT",
                        scheduler::TaskStatus::Success => "OK",
                        scheduler::TaskStatus::Failure => "FAIL",
                        scheduler::TaskStatus::Penalty => "PENT",
                    };
                    (i, task.name.clone(), status, task.last_cost)
                }).collect()
            });
        
        win.print("ID   NAME          STATUS    COST\n");
        for (i, name, status, cost) in task_data {
            win.print(&format!("{:2}   {:12}  {:4}      {:8}\n", i, name, status, cost));
        }
    }

    pub fn update_browser(win: &mut compositor::Window) {
         // Browser doesn't need constant updates unless we add a progress bar
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

    pub fn update_nano(win: &mut compositor::Window, status: &str) {
        let w = win.width;
        let h = win.height;
        
        // 1. Draw Status Bar (White background, black text)
        win.draw_rect(2, h - 50, w - 4, 18, 0xFFFFFFFF);
        win.print_fixed(5, h - 48, status, 0xFF000000); // Black text on white
        
        // 2. Draw Shortcut Menu (Black background, white text)
        win.draw_rect(2, h - 32, w - 4, 30, 0xFF000000);
        
        // Row 1
        win.print_fixed(5, h - 30, "^G Help  ^O WriteOut ^W WhereIs ^K Cut    ^J Justify ^C CurPos", 0xFFFFFFFF);
        // Row 2
        win.print_fixed(5, h - 15, "^X Exit  ^R ReadFile ^\u{005C} Replace ^U Uncut  ^T ToSpell ^_ GoToLine", 0xFFFFFFFF);
    }

    fn redraw_command_line(&mut self) {
        if let Some(win) = self.windows.get_mut(self.active_idx) {
            // 1. Clean up the text buffer and the screen
            win.truncate_text_buffer(self.prompt_start_idx);
            win.cursor_x = compositor::BORDER_WIDTH + 4;
            win.cursor_y = self.prompt_start_y;
            win.clear_from(win.cursor_y);
            
            // 2. Reprint the prompt and the full command
            win.print("> ");
            let cmd = self.command_buffer.clone();
            win.print(&cmd);
            
            // 3. Calculate and set the correct cursor position for the insertion point
            // We do this by "re-printing" up to the insertion point
            win.cursor_x = compositor::BORDER_WIDTH + 4;
            win.cursor_y = self.prompt_start_y;
            win.draw_char_no_buf('>');
            win.draw_char_no_buf(' ');
            let chars: alloc::vec::Vec<char> = self.command_buffer.chars().collect();
            for i in 0..self.insertion_point {
                if i < chars.len() {
                    win.draw_char_no_buf(chars[i]);
                }
            }
        }
    }
}

lazy_static! {
    pub static ref SHELL: Mutex<Option<Shell>> = Mutex::new(None);
}

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
    x86_64::instructions::interrupts::without_interrupts(|| {
        if let Some(ref mut shell) = *SHELL.lock() {
            shell.print("\nAPP EXECUTION SUCCESSFUL!\n");
            shell.print("Syscall 0x80 Received.\n> ");
        }
    });

    let mut is_dragging = false;
    let mut drag_offset_x = 0usize;
    let mut drag_offset_y = 0usize;

    loop {
        // 1. Run scheduler step (handles context switching)
        scheduler::step();


        // 2. GUI Logic - Mouse handling
        let (mx, my, btn) = crate::mouse::get_state();
        
        let mut draw_list: Vec<&compositor::Window> = Vec::new();
        let mut active_idx = None;

        // 1. Taskbar (Always drawn)
        let mut taskbar = compositor::Window::new(0, height - 30, width, 30, "Taskbar");
        let time = crate::time::read_rtc();
        let time_str = format!("{:02}:{:02}:{:02}", time.hours, time.minutes, time.seconds);
        taskbar.cursor_x = width - 100;
        taskbar.cursor_y = 5;
        taskbar.print(&time_str);
        draw_list.push(&taskbar);

        if let Some(mut shell_mutex_lock) = SHELL.try_lock() {
            if let Some(ref mut shell_mutex) = *shell_mutex_lock {
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
                            shell_mutex.windows.remove(new_idx);
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

                for win in &shell_mutex.windows {
                    draw_list.push(win);
                }
                active_idx = Some(shell_mutex.active_idx);
                desktop.render(&draw_list, active_idx, mx, my);
            }
        } else {
            // Fallback rendering
            desktop.render(&draw_list, None, mx, my);
        }

    }
}

pub fn shell_task() {
    let initial_shell = Shell::new();
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut shell_opt = SHELL.lock();
        if shell_opt.is_none() {
            *shell_opt = Some(initial_shell);
        }
    });

    loop {
        let mut work_done = false;
        if let Some(mut shell_mutex) = SHELL.try_lock() {
            if let Some(ref mut shell) = *shell_mutex {
                shell.run();
                work_done = true;
            }
        }

        if work_done {
            unsafe { core::arch::asm!("int 0x80", in("rax") 3); }
        } else {
            core::hint::spin_loop();
        }
    }
}
