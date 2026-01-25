use noto_sans_mono_bitmap::{get_raster, RasterizedChar, FontWeight, RasterHeight};
use spin::Mutex;
use lazy_static::lazy_static;
use crate::logger;

// --- CONFIGURATION ---
const LINE_SPACING: usize = 2;
const LETTER_SPACING: usize = 0;
const BORDER_PADDING: usize = 10;
const CHAR_WIDTH_GUESS: usize = 9; // Approximate width for backspacing

// --- THE WRITER STRUCT ---
pub struct Writer {
    pub video_ptr: *mut u32,
    pub width: usize,
    pub height: usize,
    pub pitch: usize,
    pub cursor_x: usize,
    pub cursor_y: usize,
}

// SAFETY WAIVER:
// We promise the compiler that we will only access this via Mutex.
unsafe impl Send for Writer {}
unsafe impl Sync for Writer {}

// --- GLOBAL INSTANCE ---
lazy_static! {
    pub static ref WRITER: Mutex<Option<Writer>> = Mutex::new(None);
}

impl Writer {
    pub fn init(video_ptr: *mut u32, width: usize, height: usize, pitch: usize) {
        let mut writer = WRITER.lock();
        *writer = Some(Writer {
            video_ptr,
            width,
            height,
            pitch,
            cursor_x: BORDER_PADDING,
            cursor_y: BORDER_PADDING,
        });
    }

    // Erase the whole screen to Chronos Blue
    pub fn clear(&mut self) {
        for y in 0..self.height {
            for x in 0..self.width {
                unsafe {
                    let offset = y * self.pitch + x;
                    *self.video_ptr.add(offset) = 0x00102040; // Deep Blue Theme
                }
            }
        }
        self.cursor_x = BORDER_PADDING;
        self.cursor_y = BORDER_PADDING;
    }

    pub fn write_char(&mut self, c: char) {
        match c {
            '\n' => self.new_line(),
            '\x08' => self.backspace(), // Handle Backspace (ASCII 0x08)
            char => {
                // Wrap if we hit the right edge
                if self.cursor_x + 10 >= self.width {
                    self.new_line();
                }
                self.draw_raster_char(char);
            }
        }
    }

    pub fn write_string(&mut self, s: &str) {
        for c in s.chars() {
            self.write_char(c);
        }
    }
    pub fn direct_print(&mut self, text: &str) {
        for c in text.chars() {
            self.write_char(c);
        }
    }    

    fn new_line(&mut self) {
        self.cursor_y += 16 + LINE_SPACING; // Move down by font height
        self.cursor_x = BORDER_PADDING;

        // Simple scrolling check (if we go off bottom, just reset to top for now)
        // A real OS would scroll the memory buffer.
        if self.cursor_y + 20 > self.height {
             self.clear();
        }
    }

    fn backspace(&mut self) {
        // Only backspace if we aren't at the start of the line
        if self.cursor_x >= CHAR_WIDTH_GUESS {
            self.cursor_x -= CHAR_WIDTH_GUESS;
            
            // Overwrite the character spot with Background Blue
            for y in 0..16 {
                for x in 0..CHAR_WIDTH_GUESS {
                    unsafe {
                        let offset = (self.cursor_y + y) * self.pitch + (self.cursor_x + x);
                        if (self.cursor_x + x) < self.width && (self.cursor_y + y) < self.height {
                            *self.video_ptr.add(offset) = 0x00102040; 
                        }
                    }
                }
            }
        }
    }

    fn draw_raster_char(&mut self, c: char) {
        // 1. Get the bitmap data for the character
        let raster = get_raster(c, FontWeight::Regular, RasterHeight::Size16).unwrap_or(
            get_raster('?', FontWeight::Regular, RasterHeight::Size16).unwrap()
        );

        // 2. Draw pixels
        for (y, row) in raster.raster().iter().enumerate() {
            for (x, byte) in row.iter().enumerate() {
                // *byte is brightness (0-255)
                if *byte > 0 {
                    let pixel_x = self.cursor_x + x;
                    let pixel_y = self.cursor_y + y;
                    
                    if pixel_x < self.width && pixel_y < self.height {
                        unsafe {
                            let offset = pixel_y * self.pitch + pixel_x;
                            // Simple text color (White)
                            let intensity = *byte as u32;
                            // Mix intensity with white (0xFFFFFF)
                            let color = (intensity << 16) | (intensity << 8) | intensity;
                            *self.video_ptr.add(offset) = color;
                        }
                    }
                }
            }
        }
        // 3. Advance cursor
        self.cursor_x += raster.width() + LETTER_SPACING;
    }
}

// Helper to print from anywhere
// Helper to print from anywhere
pub fn print(s: &str) {
    // 1. Log it
    logger::log(s);
    
    // 2. Force draw it (for debugging/panics)
    // We use try_lock to avoid deadlocks in interrupts
    if let Some(mut w) = WRITER.try_lock() {
        if let Some(writer) = w.as_mut() {
             writer.direct_print(s);
        }
    }
}