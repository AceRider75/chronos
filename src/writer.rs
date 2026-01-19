use noto_sans_mono_bitmap::{get_raster, RasterizedChar, FontWeight, RasterHeight};
use spin::Mutex;
use lazy_static::lazy_static;

// Constants for the screen dimensions (we'll make these dynamic later)
const LINE_SPACING: usize = 2;
const LETTER_SPACING: usize = 0;
const BORDER_PADDING: usize = 10;

pub struct Writer {
    pub video_ptr: *mut u32,
    pub width: usize,
    pub height: usize,
    pub pitch: usize,
    pub cursor_x: usize,
    pub cursor_y: usize,
}

unsafe impl Send for Writer {}
unsafe impl Sync for Writer {}

// Global Writer Instance (Thread-safe)
// We use a Mutex because interrupts might try to print at the same time as main loop
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

    // Erase screen to Chronos Blue
    pub fn clear(&mut self) {
        for y in 0..self.height {
            for x in 0..self.width {
                unsafe {
                    let offset = y * self.pitch + x;
                    *self.video_ptr.add(offset) = 0x00102040;
                }
            }
        }
        self.cursor_x = BORDER_PADDING;
        self.cursor_y = BORDER_PADDING;
    }

    pub fn write_char(&mut self, c: char) {
        match c {
            '\n' => self.new_line(),
            char => {
                // If we are off the edge, new line
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

    fn new_line(&mut self) {
        self.cursor_y += 16 + LINE_SPACING; // 16 is font height
        self.cursor_x = BORDER_PADDING;
    }

    fn draw_raster_char(&mut self, c: char) {
        // Get the pixel data for the character
        let raster = get_raster(c, FontWeight::Regular, RasterHeight::Size16).unwrap_or(
            get_raster('?', FontWeight::Regular, RasterHeight::Size16).unwrap()
        );

        for (y, row) in raster.raster().iter().enumerate() {
            for (x, byte) in row.iter().enumerate() {
                // Determine pixel intensity (Anti-aliasing)
                // *byte is the brightness (0-255)
                if *byte > 0 {
                    let pixel_x = self.cursor_x + x;
                    let pixel_y = self.cursor_y + y;
                    
                    if pixel_x < self.width && pixel_y < self.height {
                        unsafe {
                            let offset = pixel_y * self.pitch + pixel_x;
                            // Draw White Text (0xFFFFFF)
                            // We use the byte value for simple alpha blending logic
                            let intensity = *byte as u32;
                            let color = (intensity << 16) | (intensity << 8) | intensity;
                            *self.video_ptr.add(offset) = color;
                        }
                    }
                }
            }
        }
        self.cursor_x += raster.width() + LETTER_SPACING;
    }
}

// Helper function to print easily from other files
pub fn print(s: &str) {
    // Disable interrupts while printing to prevent deadlocks!
    x86_64::instructions::interrupts::without_interrupts(|| {
        if let Some(mut writer) = WRITER.lock().as_mut() {
            writer.write_string(s);
        }
    });
}