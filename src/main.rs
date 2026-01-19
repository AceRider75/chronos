#![no_std]
#![no_main]

use limine::request::FramebufferRequest;
use limine::BaseRevision;

// 1. SET THE BASE REVISION
#[used]
static BASE_REVISION: BaseRevision = BaseRevision::new();

// 2. REQUEST THE FRAMEBUFFER
#[used]
static FRAMEBUFFER_REQUEST: FramebufferRequest = FramebufferRequest::new();

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    // 3. CHECK THE RESPONSE
    if let Some(framebuffer_response) = FRAMEBUFFER_REQUEST.get_response() {
        
        if let Some(framebuffer) = framebuffer_response.framebuffers().next() {
            
            // FIX: addr() returns a raw pointer (*mut u8) directly.
            // We just cast it to *mut u32 for pixel manipulation.
            let video_ptr = framebuffer.addr() as *mut u32;
            
            let width = framebuffer.width() as usize;
            let height = framebuffer.height() as usize;
            let pitch = framebuffer.pitch() as usize / 4;

            // DRAW BLUE BACKGROUND
            for y in 0..height {
                for x in 0..width {
                    let pixel_offset = y * pitch + x;
                    unsafe {
                        *video_ptr.add(pixel_offset) = 0x00102040; // Deep Blue
                    }
                }
            }

            // DRAW RED LINE (Visual Semantics)
            for y in 0..10 {
                for x in 0..width {
                    let pixel_offset = y * pitch + x;
                    unsafe {
                        *video_ptr.add(pixel_offset) = 0x00FF0000; // Bright Red
                    }
                }
            }
        }
    }

    loop {
        core::hint::spin_loop();
    }
}