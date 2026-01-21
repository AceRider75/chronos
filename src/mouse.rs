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
    screen_width: usize,
    screen_height: usize,
}

lazy_static! {
    pub static ref MOUSE: Mutex<Mouse> = Mutex::new(Mouse {
        byte_cycle: 0,
        packet: [0; 3],
        x: 512,
        y: 384,
        screen_width: 1024,
        screen_height: 768,
    });
}

pub fn init(width: usize, height: usize) {
    let mut mouse = MOUSE.lock();
    mouse.screen_width = width;
    mouse.screen_height = height;
    
    // CRITICAL: Disable interrupts during setup!
    // We don't want the Interrupt Handler stealing the ACK bytes.
    x86_64::instructions::interrupts::without_interrupts(|| {
        unsafe {
            let mut status_port = Port::<u8>::new(STATUS_PORT);
            let mut command_port = Port::<u8>::new(COMMAND_PORT);
            let mut data_port = Port::<u8>::new(DATA_PORT);

            // 1. Enable Aux Device (Mouse)
            wait_for_write(&mut status_port);
            command_port.write(0xA8);

            // 2. Enable Interrupts (IRQ 12)
            wait_for_write(&mut status_port);
            command_port.write(0x20); // Read Config
            wait_for_read(&mut status_port);
            let mut status = data_port.read();
            status |= 2; // Set Bit 1 (Enable IRQ 12)
            wait_for_write(&mut status_port);
            command_port.write(0x60); // Write Config
            wait_for_write(&mut status_port);
            data_port.write(status);

            // 3. Set Defaults
            mouse_write(&mut status_port, &mut command_port, &mut data_port, 0xF6);
            let _ack = mouse_read(&mut status_port, &mut data_port);

            // 4. Enable Streaming
            mouse_write(&mut status_port, &mut command_port, &mut data_port, 0xF4);
            let _ack = mouse_read(&mut status_port, &mut data_port);
        }
    });
}

// Safer wait with Timeout
unsafe fn wait_for_write(port: &mut Port<u8>) {
    let mut timeout = 100000;
    while (port.read() & 0x02) != 0 { 
        timeout -= 1;
        if timeout == 0 { return; } // Give up instead of freezing
    }
}

unsafe fn wait_for_read(port: &mut Port<u8>) {
    let mut timeout = 100000;
    while (port.read() & 0x01) == 0 { 
        timeout -= 1;
        if timeout == 0 { return; } 
    }
}

unsafe fn mouse_write(status: &mut Port<u8>, cmd: &mut Port<u8>, data: &mut Port<u8>, byte: u8) {
    wait_for_write(status);
    cmd.write(0xD4); // Tell controller next byte is for mouse
    wait_for_write(status);
    data.write(byte);
}

unsafe fn mouse_read(status: &mut Port<u8>, data: &mut Port<u8>) -> u8 {
    wait_for_read(status);
    data.read()
}

pub fn handle_interrupt() {
    let mut port = Port::<u8>::new(DATA_PORT);
    let byte = unsafe { port.read() };
    
    let mut mouse = MOUSE.lock();
    
    match mouse.byte_cycle {
        0 => {
            // First byte flags check (Bit 3 should be 1)
            // Note: Some mice are buggy, but this is standard.
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

            // Handle sign bits (9-bit signed integers)
            if (state & 0x10) != 0 { dx -= 256; }
            if (state & 0x20) != 0 { dy -= 256; }

            // Update Position (Invert Y because screens go down)
            let x = (mouse.x as i32 + dx).clamp(0, (mouse.screen_width - 5) as i32);
            let y = (mouse.y as i32 - dy).clamp(0, (mouse.screen_height - 5) as i32);
            
            mouse.x = x as usize;
            mouse.y = y as usize;

            draw_cursor(mouse.x, mouse.y);
        }
        _ => mouse.byte_cycle = 0,
    }
}

fn draw_cursor(x: usize, y: usize) {
    if let Some(mut w) = writer::WRITER.lock().as_mut() {
        // Draw a simple 5x5 White Box
        // WARNING: This is destructive. It leaves a trail.
        // A real OS saves the background before drawing.
        for i in 0..5 {
            for j in 0..5 {
                unsafe {
                    let offset = (y + i) * w.pitch + (x + j);
                    if offset < w.width * w.height {
                        *w.video_ptr.add(offset) = 0xFFFFFFFF;
                    }
                }
            }
        }
    }
}