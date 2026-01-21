use alloc::vec::Vec;
use alloc::vec;
use crate::{writer, mouse};

pub struct Window {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
    pub data: Vec<u32>, // The pixel data for this window
}

impl Window {
    pub fn new(x: usize, y: usize, w: usize, h: usize, color: u32) -> Self {
        let size = w * h;
        // Create a solid color window for now
        let data = vec![color; size];
        Window { x, y, width: w, height: h, data }
    }
}

pub struct Compositor {
    width: usize,
    height: usize,
    backbuffer: Vec<u32>,
    windows: Vec<Window>,
}

impl Compositor {
    pub fn new(width: usize, height: usize) -> Self {
        let size = width * height;
        // Allocate the backbuffer (RAM)
        let backbuffer = vec![0x00102040; size]; // Default Chronos Blue
        
        Compositor {
            width,
            height,
            backbuffer,
            windows: Vec::new(),
        }
    }

    pub fn add_window(&mut self, win: Window) {
        self.windows.push(win);
    }

    // THE RENDER LOOP
    pub fn render(&mut self) {
        // 1. Clear Backbuffer to Background Color
        // (Optimized fill)
        self.backbuffer.fill(0x00102040); // Deep Blue

        // 2. Draw Windows
        for win in &self.windows {
            for row in 0..win.height {
                for col in 0..win.width {
                    // Calculate position on screen
                    let screen_y = win.y + row;
                    let screen_x = win.x + col;

                    if screen_x < self.width && screen_y < self.height {
                        let screen_idx = screen_y * self.width + screen_x;
                        let win_idx = row * win.width + col;
                        
                        // Copy pixel from Window to Backbuffer
                        self.backbuffer[screen_idx] = win.data[win_idx];
                    }
                }
            }
        }

        // 3. Draw Mouse
        let (mx, my) = mouse::get_position();
        // Simple 10x10 White Box Cursor
        for i in 0..10 {
            for j in 0..10 {
                let sy = my + i;
                let sx = mx + j;
                if sx < self.width && sy < self.height {
                    let idx = sy * self.width + sx;
                    // Border effect
                    if i==0 || i==9 || j==0 || j==9 {
                        self.backbuffer[idx] = 0xFF000000; // Black
                    } else {
                        self.backbuffer[idx] = 0xFFFFFFFF; // White
                    }
                }
            }
        }

        // 4. THE FLIP (RAM -> VRAM)
        // We lock the writer and copy the entire buffer at once.
        if let Some(mut w) = writer::WRITER.lock().as_mut() {
            // Safety: We assume backbuffer size matches screen size (setup in init)
            // Copy 4 bytes at a time (u32)
            unsafe {
                core::ptr::copy_nonoverlapping(
                    self.backbuffer.as_ptr(),
                    w.video_ptr,
                    self.backbuffer.len()
                );
            }
        }
    }
}