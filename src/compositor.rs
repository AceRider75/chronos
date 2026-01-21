use alloc::vec::Vec;
use alloc::vec;
use crate::{writer, mouse};
use noto_sans_mono_bitmap::{get_raster, FontWeight, RasterHeight}; // Import Font

pub struct Window {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
    pub data: Vec<u32>,
    // Cursor for text inside the window
    pub cursor_x: usize,
    pub cursor_y: usize,
}

impl Window {
    pub fn new(x: usize, y: usize, w: usize, h: usize, color: u32) -> Self {
        let size = w * h;
        let data = vec![color; size];
        Window { 
            x, y, width: w, height: h, data,
            cursor_x: 5, cursor_y: 5 
        }
    }

    pub fn clear(&mut self, color: u32) {
        self.data.fill(color);
        self.cursor_x = 5;
        self.cursor_y = 5;
    }

    // DRAW CHAR INTO WINDOW BUFFER
    pub fn draw_char(&mut self, c: char) {
        match c {
            '\n' => {
                self.cursor_x = 5;
                self.cursor_y += 16 + 2; // Line height
            }
            '\x08' => { // Backspace
                if self.cursor_x >= 9 {
                    self.cursor_x -= 9;
                    // Draw black box to erase
                    self.draw_rect(self.cursor_x, self.cursor_y, 9, 16, 0xFF000000);
                }
            }
            _ => {
                let raster = get_raster(c, FontWeight::Regular, RasterHeight::Size16).unwrap_or(
                    get_raster('?', FontWeight::Regular, RasterHeight::Size16).unwrap()
                );
                
                // Wrap text if too wide
                if self.cursor_x + raster.width() >= self.width {
                    self.cursor_x = 5;
                    self.cursor_y += 18;
                }

                // Check scrolling (Simple: just clear and reset if full)
                // Real OS would scroll the buffer up.
                if self.cursor_y + 16 >= self.height {
                    self.clear(0xFF000000);
                }

                for (y, row) in raster.raster().iter().enumerate() {
                    for (x, byte) in row.iter().enumerate() {
                        if *byte > 0 {
                            let px = self.cursor_x + x;
                            let py = self.cursor_y + y;
                            if px < self.width && py < self.height {
                                let idx = py * self.width + px;
                                // White text
                                self.data[idx] = 0xFFFFFFFF;
                            }
                        }
                    }
                }
                self.cursor_x += raster.width();
            }
        }
    }

    pub fn print(&mut self, text: &str) {
        for c in text.chars() {
            self.draw_char(c);
        }
    }
    pub fn contains(&self, px: usize, py: usize) -> bool {
        px >= self.x && px < self.x + self.width &&
        py >= self.y && py < self.y + self.height
    }    

    fn draw_rect(&mut self, x: usize, y: usize, w: usize, h: usize, color: u32) {
        for i in 0..h {
            for j in 0..w {
                let px = x + j;
                let py = y + i;
                if px < self.width && py < self.height {
                    let idx = py * self.width + px;
                    self.data[idx] = color;
                }
            }
        }
    }

}

pub struct Compositor {
    width: usize,
    height: usize,
    backbuffer: Vec<u32>,
    // We remove the internal 'windows' Vec. 
    // We will render windows passed to us explicitly.
}

impl Compositor {
    pub fn new(width: usize, height: usize) -> Self {
        let size = width * height;
        let backbuffer = vec![0x00102040; size];
        Compositor { width, height, backbuffer }
    }

    // New API: Takes a list of windows to draw
    pub fn render(&mut self, windows: &[&Window]) {
        // 1. Clear
        self.backbuffer.fill(0x00102040);

        // 2. Draw Windows
        for win in windows {
            for row in 0..win.height {
                for col in 0..win.width {
                    let screen_y = win.y + row;
                    let screen_x = win.x + col;

                    if screen_x < self.width && screen_y < self.height {
                        let idx = screen_y * self.width + screen_x;
                        let win_idx = row * win.width + col;
                        self.backbuffer[idx] = win.data[win_idx];
                    }
                }
            }
        }

        // 3. Draw Mouse
        let (mx, my) = mouse::get_position();
        for i in 0..10 {
            for j in 0..10 {
                let sy = my + i;
                let sx = mx + j;
                if sx < self.width && sy < self.height {
                    let idx = sy * self.width + sx;
                    let color = if i==0||i==9||j==0||j==9 { 0xFF000000 } else { 0xFFFFFFFF };
                    self.backbuffer[idx] = color;
                }
            }
        }

        // 4. Flip
        if let Some(mut w) = writer::WRITER.lock().as_mut() {
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