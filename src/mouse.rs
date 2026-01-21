use x86_64::instructions::port::Port;
use spin::Mutex;
use lazy_static::lazy_static;

const DATA_PORT: u16 = 0x60;
const STATUS_PORT: u16 = 0x64;
const COMMAND_PORT: u16 = 0x64;

pub struct Mouse {
    byte_cycle: u8,
    packet: [u8; 3],
    pub x: usize,
    pub y: usize,
    pub left_button: bool, // <--- NEW
    screen_width: usize,
    screen_height: usize,
    saved_background: [u32; 100], 
    first_draw: bool,
}

lazy_static! {
    pub static ref MOUSE: Mutex<Mouse> = Mutex::new(Mouse {
        byte_cycle: 0,
        packet: [0; 3],
        x: 512,
        y: 384,
        left_button: false, // <--- Init false
        screen_width: 1024,
        screen_height: 768,
        saved_background: [0; 100],
        first_draw: true,
    });
}

pub fn get_state() -> (usize, usize, bool) {
    if let Some(m) = MOUSE.try_lock() {
        (m.x, m.y, m.left_button)
    } else {
        (0, 0, false)
    }
}


// Allow the Compositor to ask where the mouse is
pub fn get_position() -> (usize, usize) {
    // We use try_lock to prevent deadlocks. If locked, return 0,0 (flicker is better than freeze)
    if let Some(m) = MOUSE.try_lock() {
        (m.x, m.y)
    } else {
        (0, 0)
    }
}

pub fn init(width: usize, height: usize) {
    let mut mouse = MOUSE.lock();
    mouse.screen_width = width;
    mouse.screen_height = height;
    
    // Hardware Init (Keep the interrupt disable block from Phase 17)
    x86_64::instructions::interrupts::without_interrupts(|| {
        unsafe {
            let mut status = Port::<u8>::new(STATUS_PORT);
            let mut cmd = Port::<u8>::new(COMMAND_PORT);
            let mut data = Port::<u8>::new(DATA_PORT);

            wait_write(&mut status); cmd.write(0xA8);
            wait_write(&mut status); cmd.write(0x20);
            wait_read(&mut status);
            let config = data.read() | 2;
            wait_write(&mut status); cmd.write(0x60);
            wait_write(&mut status); data.write(config);

            write_mouse(&mut status, &mut cmd, &mut data, 0xF6);
            read_mouse(&mut status, &mut data);
            
            write_mouse(&mut status, &mut cmd, &mut data, 0xF4);
            read_mouse(&mut status, &mut data);
        }
    });
}

// Keep helpers (wait_write, wait_read, write_mouse, read_mouse) exactly as they were in Phase 17
unsafe fn wait_write(port: &mut Port<u8>) {
    let mut timeout = 100000;
    while (port.read() & 0x02) != 0 { timeout -= 1; if timeout == 0 { return; } }
}
unsafe fn wait_read(port: &mut Port<u8>) {
    let mut timeout = 100000;
    while (port.read() & 0x01) == 0 { timeout -= 1; if timeout == 0 { return; } }
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
    
    // Use try_lock to prevent deadlocks with the main loop reading state
    if let Some(mut mouse) = MOUSE.try_lock() {
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

                // Update Position
                let x = (mouse.x as i32 + dx).clamp(0, (mouse.screen_width - 5) as i32);
                let y = (mouse.y as i32 - dy).clamp(0, (mouse.screen_height - 5) as i32);
                
                mouse.x = x as usize;
                mouse.y = y as usize;
                
                // NEW: Update Button State (Bit 0 of Byte 0)
                mouse.left_button = (state & 0x01) != 0;

                // (Draw logic was moved to compositor, so we are done)
            }
            _ => mouse.byte_cycle = 0,
        }
    }
}