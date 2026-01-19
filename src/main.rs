#![feature(abi_x86_interrupt)]
#![no_std]
#![no_main]

use limine::request::FramebufferRequest;
use limine::BaseRevision;
use core::arch::x86_64::_rdtsc;

mod interrupts; // IMPORT THE NEW MODULE

#[used]
static BASE_REVISION: BaseRevision = BaseRevision::new();

#[used]
static FRAMEBUFFER_REQUEST: FramebufferRequest = FramebufferRequest::new();

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    // 1. INITIALIZE INTERRUPTS
    interrupts::init_idt();

    // 2. FIRE A TEST INTERRUPT (Breakpoint)
    // If the OS doesn't crash here, it means our IDT works!
    x86_64::instructions::interrupts::int3();

    let framebuffer_response = FRAMEBUFFER_REQUEST.get_response().unwrap();
    let framebuffer = framebuffer_response.framebuffers().next().unwrap();
    
    let video_ptr = framebuffer.addr() as *mut u32;
    let width = framebuffer.width() as usize;
    let height = framebuffer.height() as usize;
    let pitch = framebuffer.pitch() as usize / 4;

    // VISUAL CONFIRMATION:
    // Draw a WHITE line at the very top to prove we survived the interrupt.
    for x in 0..width {
        unsafe {
            *video_ptr.add(x) = 0xFFFFFFFF; // White
        }
    }

    let cycle_budget: u64 = 2_500_000; 
    let mut frame_count: u64 = 0;
    
    loop {
        let start_time = unsafe { _rdtsc() };

        let blue_val = (frame_count % 255) as u32;
        let bg_color = 0x00102000 | blue_val; 

        // Start from y=50
        for y in 50..height { 
            for x in 0..width {
                unsafe {
                    let offset = y * pitch + x;
                    *video_ptr.add(offset) = bg_color;
                }
            }
        }

        let end_time = unsafe { _rdtsc() };
        let elapsed = end_time - start_time;

        let mut bar_width = ((elapsed as u128 * width as u128) / cycle_budget as u128) as usize;
        if bar_width > width { bar_width = width; }

        let usage_color = if bar_width < width / 2 {
            0x0000FF00 
        } else if bar_width < width {
            0x00FFFF00 
        } else {
            0x00FF0000 
        };

        // Draw Fuel Gauge
        for y in 10..50 { // Draw below the white interrupt line
            for x in 0..width {
                unsafe {
                    let offset = y * pitch + x;
                    if x < bar_width {
                        *video_ptr.add(offset) = usage_color;
                    } else {
                        *video_ptr.add(offset) = 0x00333333;
                    }
                }
            }
        }

        frame_count = frame_count.wrapping_add(1);
    }
}