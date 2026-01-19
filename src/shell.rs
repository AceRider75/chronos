use crate::{input, writer};
use alloc::string::String;
use alloc::vec::Vec;

pub struct Shell {
    command_buffer: String,
}

impl Shell {
    pub fn new() -> Self {
        Shell {
            command_buffer: String::new(),
        }
    }

    // This function will be called by the Scheduler repeatedly
    pub fn run(&mut self) {
        // Check if there are keys in the buffer
        while let Some(c) = input::pop_key() {
            match c {
                '\n' => {
                    // User pressed Enter
                    writer::print("\n"); // New line on screen
                    self.execute_command();
                    self.command_buffer.clear();
                    writer::print("> "); // Prompt for next command
                }
                '\x08' => {
                    // Backspace (ASCII 0x08)
                    if !self.command_buffer.is_empty() {
                        self.command_buffer.pop();
                        // Visual hack: move cursor back, print space, move back
                        // Since we don't have full terminal control, we'll just print a generic backspace indicator for now
                        // or just ignore visual deletion until we have a better terminal.
                        // Ideally: writer::backspace();
                        writer::print("\x08"); // Try printing the char, writer might handle it?
                    }
                }
                _ => {
                    // Normal character
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
        match cmd {
            "help" => {
                writer::print("Chronos Shell v0.1\n");
                writer::print("Commands: help, ver, clear, budget\n");
            },
            "ver" => writer::print("Chronos OS v0.5 - Phase 6\n"),
            "clear" => {
                if let Some(w) = writer::WRITER.lock().as_mut() {
                    w.clear();
                    w.cursor_y = 10;
                }
            },
            "budget" => writer::print("TODO: Adjust budget via command\n"),
            "" => {}, // Empty enter
            _ => {
                writer::print("Unknown command: ");
                writer::print(cmd);
                writer::print("\n");
            }
        }
    }
}

// Global Shell Instance
use spin::Mutex;
use lazy_static::lazy_static;
lazy_static! {
    pub static ref SHELL: Mutex<Shell> = Mutex::new(Shell::new());
}

// The Job function for the Scheduler
pub fn shell_task() {
    SHELL.lock().run();
}