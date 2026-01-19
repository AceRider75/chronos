#![no_std]
#![no_main]

use limine::request::FramebufferRequest;
use limine::BaseRevision;
use core::arch::x86_64::_rdtsc;

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
    let framebuffer_response = FRAMEBUFFER_REQUEST.get_response().unwrap();
    let framebuffer = framebuffer_response.framebuffers().next().unwrap();
    
    let video_ptr = framebuffer.addr() as *mut u32;
    let width = framebuffer.width() as usize;
    let height = framebuffer.height() as usize;
    let pitch = framebuffer.pitch() as usize / 4;

    // THE BUDGET: Keep it at the "Edge of Chaos" value you found
    let cycle_budget: u64 = 2_500_000; 

    let mut frame_count: u64 = 0;
    
    loop {
        let start_time = unsafe { _rdtsc() };

        // -------------------------------------------------------------
        // 1. THE WORKLOAD (Drawing the Blue Background)
        // -------------------------------------------------------------
        let blue_val = (frame_count % 255) as u32;
        let bg_color = 0x00102000 | blue_val; 

        // Draw the background (skipping top 50 lines for the Fuel Gauge)
        for y in 50..height { 
            for x in 0..width {
                unsafe {
                    let offset = y * pitch + x;
                    *video_ptr.add(offset) = bg_color;
                }
            }
        }

        // -------------------------------------------------------------
        // 2. CALCULATE TIME COST
        // -------------------------------------------------------------
        let end_time = unsafe { _rdtsc() };
        let elapsed = end_time - start_time;

        // Math: (elapsed / budget) * width
        // We use u128 to prevent overflow during the multiplication
        let mut bar_width = ((elapsed as u128 * width as u128) / cycle_budget as u128) as usize;
        
        // Cap the bar width so it doesn't wrap around the screen if we fail hard
        if bar_width > width {
            bar_width = width;
        }

        // -------------------------------------------------------------
        // 3. DRAW THE SEMANTIC FUEL GAUGE
        // -------------------------------------------------------------
        
        // Determine Color based on usage
        // < 50% = Green, > 50% = Yellow, > 100% = Red
        let usage_color = if bar_width < width / 2 {
            0x0000FF00 // Green
        } else if bar_width < width {
            0x00FFFF00 // Yellow
        } else {
            0x00FF0000 // Red (Failure)
        };

        for y in 0..50 { // Make the bar 50px tall so it's obvious
            for x in 0..width {
                unsafe {
                    let offset = y * pitch + x;
                    if x < bar_width {
                        *video_ptr.add(offset) = usage_color; // Used Time
                    } else {
                        *video_ptr.add(offset) = 0x00333333; // Dark Grey (Free Time)
                    }
                }
            }
        }

        frame_count = frame_count.wrapping_add(1);
    }
}