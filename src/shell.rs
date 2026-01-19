use crate::{input, writer, fs}; // Import the new FS module
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;
use lazy_static::lazy_static;

// --- THE SHELL STRUCTURE ---
pub struct Shell {
    command_buffer: String,
}

impl Shell {
    pub fn new() -> Self {
        Shell {
            command_buffer: String::new(),
        }
    }

    // This function is called repeatedly by the Scheduler
    pub fn run(&mut self) {
        // Drain the Input Buffer (process all queued keys)
        while let Some(c) = input::pop_key() {
            match c {
                '\n' => {
                    // User pressed Enter: Execute and reset
                    writer::print("\n");
                    self.execute_command();
                    self.command_buffer.clear();
                    writer::print("> "); // Prompt for next command
                }
                '\x08' => {
                    // Backspace (ASCII 0x08)
                    if !self.command_buffer.is_empty() {
                        self.command_buffer.pop();
                        // Send backspace char to writer for visual handling
                        writer::print("\x08"); 
                    }
                }
                _ => {
                    // Standard character: Append and echo
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
        
        // Split string into command + arguments (e.g. "cat welcome.txt")
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        
        if parts.is_empty() { return; }

        match parts[0] {
            "help" => {
                writer::print("Chronos Shell v0.2\n");
                writer::print("Commands:\n");
                writer::print("  help       - Show this menu\n");
                writer::print("  ver        - Show OS version\n");
                writer::print("  clear      - Clear screen\n");
                writer::print("  ls         - List files in Ramdisk\n");
                writer::print("  cat <file> - Print file content\n");
            },
            "ver" => {
                writer::print("Chronos OS v0.6\n");
                writer::print("Phase 7: Persistence (Ramdisk)\n");
            },
            "clear" => {
                // We need to lock the writer manually to access special methods
                if let Some(w) = writer::WRITER.lock().as_mut() {
                    w.clear();
                    // Reset cursor to top, but leave room for status bar if you want
                    w.cursor_y = 10; 
                }
            },
            "ls" => {
                writer::print("--- Ramdisk Files ---\n");
                let files = fs::list_files();
                
                if files.is_empty() {
                    writer::print("(Empty)\n");
                } else {
                    for file in files {
                        writer::print("- ");
                        writer::print(&file.name);
                        writer::print("\n");
                    }
                }
            },
            "cat" => {
                if parts.len() < 2 {
                    writer::print("Usage: cat <filename>\n");
                } else {
                    let filename = parts[1];
                    // Search for the file
                    if let Some(content) = fs::read_file(filename) {
                        writer::print("--- BEGIN FILE ---\n");
                        writer::print(&content);
                        if !content.ends_with('\n') { writer::print("\n"); }
                        writer::print("--- END FILE ---\n");
                    } else {
                        writer::print("Error: File not found.\n");
                    }
                }
            },
            _ => {
                writer::print("Unknown command: ");
                writer::print(cmd);
                writer::print("\n");
            }
        }
    }
}

// --- GLOBAL SHELL INSTANCE ---
lazy_static! {
    pub static ref SHELL: Mutex<Shell> = Mutex::new(Shell::new());
}

// --- SCHEDULER ENTRY POINT ---
pub fn shell_task() {
    SHELL.lock().run();
}