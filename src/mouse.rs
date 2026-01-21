use x86_64::instructions::port::Port;
use spin::Mutex;
use lazy_static::lazy_static;
use crate::writer;

const DATA_PORT: u16 = 0x60;
const STATUS_PORT: u16 = 0x64;
const COMMAND_PORT: u16 = 0x64;

pub struct Mouse {
    byte_cycle: u8,
    packet: [u8; 3],
    pub x: usize,
    pub y: usize,
    pub prev_x: usize,
    pub prev_y: usize,
    screen_width: usize,
    screen_height: usize,
    // Buffer to save the background behind the cursor (10x10 = 100 pixels)
    saved_background: [u32; 100], 
    first_draw: bool,
}

lazy_static! {
    pub static ref MOUSE: Mutex<Mouse> = Mutex::new(Mouse {
        byte_cycle: 0,
        packet: [0; 3],
        x: 512,
        y: 384,
        prev_x: 512,
        prev_y: 384,
        screen_width: 1024,
        screen_height: 768,
        saved_background: [0; 100], // Black by default
        first_draw: true,
    });
}

pub fn init(width: usize, height: usize) {
    let mut mouse = MOUSE.lock();
    mouse.screen_width = width;
    mouse.screen_height = height;
    
    // Initial draw to save the first spot
    draw_cursor_logic(&mut mouse);

    x86_64::instructions::interrupts::without_interrupts(|| {
        unsafe {
            let mut status = Port::<u8>::new(STATUS_PORT);
            let mut cmd = Port::<u8>::new(COMMAND_PORT);
            let mut data = Port::<u8>::new(DATA_PORT);

            wait_write(&mut status); cmd.write(0xA8); // Enable Aux
            wait_write(&mut status); cmd.write(0x20); // Read Config
            wait_read(&mut status); 
            let config = data.read() | 2; // Enable IRQ12
            wait_write(&mut status); cmd.write(0x60); 
            wait_write(&mut status); data.write(config);

            write_mouse(&mut status, &mut cmd, &mut data, 0xF6); // Default
            let _ = read_mouse(&mut status, &mut data);
            
            write_mouse(&mut status, &mut cmd, &mut data, 0xF4); // Enable Streaming
            let _ = read_mouse(&mut status, &mut data);
        }
    });
}

unsafe fn wait_write(port: &mut Port<u8>) {
    while (port.read() & 0x02) != 0 { core::hint::spin_loop(); }
}
unsafe fn wait_read(port: &mut Port<u8>) {
    while (port.read() & 0x01) == 0 { core::hint::spin_loop(); }
}
unsafe fn write_mouse(status: &mut Port<u8>, cmd: &mut Port<u8>, data: &mut Port<u8>, byte: u8) {
    wait_write(status); cmd.write(0xD4);
    wait_write(status); data.write(byte);
}
unsafe fn read_mouse(status: &mut Port<u8>, data: &mut Port<u8>) -> u8 {
    wait_read(status); data.read()
}

pub fn handle_interrupt() {
    let mut port = Port::<u8>::new(DATA_PORT);
    let byte = unsafe { port.read() };
    
    // We lock carefully to avoid deadlocks with the writer
    let mut mouse = MOUSE.lock();
    
    match mouse.byte_cycle {
        0 => {
            if (byte & 0x08) != 0 {
                mouse.packet[0] = byte;
                mouse.byte_cycle += 1;
            }
        }
        1 => {
            mouse.packet[1] = byte;
            mouse.byte_cycle += 1;
        }
        2 => {
            mouse.packet[2] = byte;
            mouse.byte_cycle = 0;

            let state = mouse.packet[0];
            let mut dx = mouse.packet[1] as i32;
            let mut dy = mouse.packet[2] as i32;
            if (state & 0x10) != 0 { dx -= 256; }
            if (state & 0x20) != 0 { dy -= 256; }

            // Save old position
            mouse.prev_x = mouse.x;
            mouse.prev_y = mouse.y;

            // Calculate new position
            let x = (mouse.x as i32 + dx).clamp(0, (mouse.screen_width - 10) as i32);
            let y = (mouse.y as i32 - dy).clamp(0, (mouse.screen_height - 10) as i32);
            
            mouse.x = x as usize;
            mouse.y = y as usize;

            // Only redraw if moved
            if mouse.x != mouse.prev_x || mouse.y != mouse.prev_y {
                draw_cursor_logic(&mut mouse);
            }
        }
        _ => mouse.byte_cycle = 0,
    }
}

// Logic to erase old cursor and draw new one
fn draw_cursor_logic(mouse: &mut Mouse) {
    // We need to access video memory. 
    // WARNING: This locks the Writer. Ensure no one else holds this lock during an interrupt!
    if let Some(mut w) = writer::WRITER.try_lock() {
        let w = w.as_mut().unwrap(); // Unwrap the Option inside the mutex
        
        // 1. RESTORE BACKGROUND (Erase old cursor)
        if !mouse.first_draw {
            for i in 0..10 {
                for j in 0..10 {
                    unsafe {
                        let offset = (mouse.prev_y + i) * w.pitch + (mouse.prev_x + j);
                        // Read from our save buffer
                        let saved_pixel = mouse.saved_background[i * 10 + j];
                        // Write back to screen
                        *w.video_ptr.add(offset) = saved_pixel;
                    }
                }
            }
        }

        // 2. SAVE NEW BACKGROUND (Under new cursor)
        for i in 0..10 {
            for j in 0..10 {
                unsafe {
                    let offset = (mouse.y + i) * w.pitch + (mouse.x + j);
                    // Read from screen
                    let screen_pixel = *w.video_ptr.add(offset);
                    // Save to buffer
                    mouse.saved_background[i * 10 + j] = screen_pixel;
                }
            }
        }

        // 3. DRAW NEW CURSOR (White Box)
        for i in 0..10 {
            for j in 0..10 {
                // Simple border effect: Black border, White center
                let color = if i == 0 || i == 9 || j == 0 || j == 9 { 
                    0xFF000000 // Black Border
                } else { 
                    0xFFFFFFFF // White Fill 
                };

                unsafe {
                    let offset = (mouse.y + i) * w.pitch + (mouse.x + j);
                    *w.video_ptr.add(offset) = color;
                }
            }
        }
        
        mouse.first_draw = false;
    }
}