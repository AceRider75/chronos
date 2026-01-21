use alloc::vec::Vec;
use alloc::vec;
use crate::{writer, mouse};
use noto_sans_mono_bitmap::{get_raster, FontWeight, RasterHeight};

// --- STYLE CONSTANTS ---
const BORDER_COLOR: u32 = 0xFFC0C0C0; // Light Grey
const TITLE_COLOR: u32 = 0xFF000080;  // Navy Blue
const CONTENT_COLOR: u32 = 0xFF000000; // Black
const BORDER_WIDTH: usize = 2;
const TITLE_HEIGHT: usize = 20;

pub struct Window {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
    pub data: Vec<u32>,
    pub cursor_x: usize,
    pub cursor_y: usize,
    // Store title for rendering
    pub title: alloc::string::String,
}

impl Window {
    pub fn new(x: usize, y: usize, w: usize, h: usize, title: &str) -> Self {
        let size = w * h;
        let mut win = Window { 
            x, y, width: w, height: h, 
            data: vec![CONTENT_COLOR; size],
            cursor_x: BORDER_WIDTH + 4, 
            cursor_y: TITLE_HEIGHT + 4,
            title: alloc::string::String::from(title),
        };
        
        win.draw_decorations();
        win
    }

    pub fn draw_decorations(&mut self) {
        // 1. Draw Main Border (Background Fill first)
        self.data.fill(BORDER_COLOR);

        // 2. Draw Title Bar
        for y in BORDER_WIDTH..TITLE_HEIGHT {
            for x in BORDER_WIDTH..(self.width - BORDER_WIDTH) {
                let idx = y * self.width + x;
                self.data[idx] = TITLE_COLOR;
            }
        }

        // 3. Draw Content Area (Black Box)
        // Starts below Title Bar
        let content_top = TITLE_HEIGHT;
        let content_bottom = self.height - BORDER_WIDTH;
        let content_left = BORDER_WIDTH;
        let content_right = self.width - BORDER_WIDTH;

        for y in content_top..content_bottom {
            for x in content_left..content_right {
                let idx = y * self.width + x;
                self.data[idx] = CONTENT_COLOR;
            }
        }
        
        // 4. (Optional) Draw Title Text could go here if we extracted a string-draw helper
        // For now, we rely on the shell to print text, but decorations make it look like a window.
    }

    // Only clear the Black Area, don't wipe the borders!
    pub fn clear(&mut self) {
        let content_top = TITLE_HEIGHT;
        let content_bottom = self.height - BORDER_WIDTH;
        let content_left = BORDER_WIDTH;
        let content_right = self.width - BORDER_WIDTH;

        for y in content_top..content_bottom {
            for x in content_left..content_right {
                let idx = y * self.width + x;
                self.data[idx] = CONTENT_COLOR;
            }
        }
        // Reset Cursor to top-left of CONTENT area
        self.cursor_x = BORDER_WIDTH + 4;
        self.cursor_y = TITLE_HEIGHT + 4;
    }

    pub fn draw_char(&mut self, c: char) {
        match c {
            '\n' => {
                self.cursor_x = BORDER_WIDTH + 4;
                self.cursor_y += 18;
            }
            '\x08' => { // Backspace
                if self.cursor_x >= (BORDER_WIDTH + 4 + 9) {
                    self.cursor_x -= 9;
                    self.draw_rect(self.cursor_x, self.cursor_y, 9, 16, CONTENT_COLOR);
                }
            }
            _ => {
                let raster = get_raster(c, FontWeight::Regular, RasterHeight::Size16).unwrap_or(
                    get_raster('?', FontWeight::Regular, RasterHeight::Size16).unwrap()
                );
                
                // Wrap
                if self.cursor_x + raster.width() >= (self.width - BORDER_WIDTH) {
                    self.cursor_x = BORDER_WIDTH + 4;
                    self.cursor_y += 18;
                }

                // Scroll Check
                if self.cursor_y + 16 >= (self.height - BORDER_WIDTH) {
                    self.clear(); // Simple scroll = clear for now
                }

                for (y, row) in raster.raster().iter().enumerate() {
                    for (x, byte) in row.iter().enumerate() {
                        if *byte > 0 {
                            let px = self.cursor_x + x;
                            let py = self.cursor_y + y;
                            // Bounds Check
                            if px < self.width && py < self.height {
                                let idx = py * self.width + px;
                                self.data[idx] = 0xFFFFFFFF; // White Text
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

    // Hit test checks the whole window including border
    pub fn contains(&self, px: usize, py: usize) -> bool {
        px >= self.x && px < self.x + self.width &&
        py >= self.y && py < self.y + self.height
    }
    
    // Check if clicking the Title Bar (for dragging)
    pub fn is_title_bar(&self, px: usize, py: usize) -> bool {
        if !self.contains(px, py) { return false; }
        // Relative Y
        let rel_y = py - self.y;
        rel_y < TITLE_HEIGHT
    }
}

pub struct Compositor {
    width: usize,
    height: usize,
    backbuffer: Vec<u32>,
}

impl Compositor {
    pub fn new(width: usize, height: usize) -> Self {
        let size = width * height;
        let backbuffer = vec![0x00102040; size];
        Compositor { width, height, backbuffer }
    }

    pub fn render(&mut self, windows: &[&Window]) {
        self.backbuffer.fill(0x00102040); // Clear to Blue

        for win in windows {
            for row in 0..win.height {
                for col in 0..win.width {
                    let screen_y = win.y + row;
                    let screen_x = win.x + col;

                    if screen_x < self.width && screen_y < self.height {
                        let idx = screen_y * self.width + screen_x;
                        let win_idx = row * win.width + col;
                        
                        // Transparency check (if we wanted shaped windows), 
                        // but for now just copy opaque.
                        self.backbuffer[idx] = win.data[win_idx];
                    }
                }
            }
        }

        // Draw Mouse
        let (mx, my, _) = mouse::get_state();
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

        // Flip
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