#![feature(abi_x86_interrupt)]
#![no_std]
#![no_main]

use limine::request::FramebufferRequest;
use limine::BaseRevision;
use core::arch::x86_64::_rdtsc;
use core::sync::atomic::Ordering;

// MODULES
mod interrupts;
mod state;
mod writer; // The new text rendering module

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
    // -----------------------------------------------------------------------
    // 1. INITIALIZE THE NERVOUS SYSTEM (Interrupts)
    // -----------------------------------------------------------------------
    interrupts::init_idt();
    unsafe { interrupts::PICS.lock().initialize() };
    interrupts::enable_listening(); // Force PIC to listen to IRQ 0 and 1
    x86_64::instructions::interrupts::enable(); // Unmute the CPU

    // -----------------------------------------------------------------------
    // 2. INITIALIZE VISUALS (Framebuffer)
    // -----------------------------------------------------------------------
    let framebuffer_response = FRAMEBUFFER_REQUEST.get_response().unwrap();
    let framebuffer = framebuffer_response.framebuffers().next().unwrap();
    
    let video_ptr = framebuffer.addr() as *mut u32;
    let width = framebuffer.width() as usize;
    let height = framebuffer.height() as usize;
    let pitch = framebuffer.pitch() as usize / 4;

    // -----------------------------------------------------------------------
    // 3. INITIALIZE THE WRITER (Text Engine)
    // -----------------------------------------------------------------------
    writer::Writer::init(video_ptr, width, height, pitch);

    // Clear the screen to Chronos Blue first
    if let Some(mut w) = writer::WRITER.lock().as_mut() {
        w.clear();
    }

    // PRINT STARTUP LOG
    writer::print("Chronos OS v0.2\n");
    writer::print("--------------------------\n");
    writer::print("[ OK ] CPU Online\n");
    writer::print("[ OK ] IDT & PIC Initialized\n");
    writer::print("[ OK ] Framebuffer Linked\n");
    writer::print("[INFO] Press +/- to adjust Time Budget\n");
    writer::print("[INFO] System is LIVE.\n");

    let mut frame_count: u64 = 0;
    
    // -----------------------------------------------------------------------
    // 4. THE MAIN LOOP (The Heartbeat)
    // -----------------------------------------------------------------------
    loop {
        // A. READ BUDGET
        let cycle_budget = state::CYCLE_BUDGET.load(Ordering::Relaxed);
        
        // B. START CLOCK
        let start_time = unsafe { _rdtsc() };

        // C. DRAW BACKGROUND ANIMATION
        // We start at y = 200 to protect the text at the top!
        let blue_val = (frame_count % 255) as u32;
        let bg_color = 0x00102000 | blue_val; 

        for y in 200..height { 
            for x in 0..width {
                unsafe {
                    let offset = y * pitch + x;
                    *video_ptr.add(offset) = bg_color;
                }
            }
        }

        // D. STOP CLOCK
        let end_time = unsafe { _rdtsc() };
        let elapsed = end_time - start_time;

        // E. DRAW FUEL GAUGE (In the middle stripe: y=150 to y=200)
        let mut bar_width = ((elapsed as u128 * width as u128) / cycle_budget as u128) as usize;
        if bar_width > width { bar_width = width; }

        let usage_color = if bar_width < width / 2 { 
            0x0000FF00 // Green (Good)
        } else if bar_width < width {
            0x00FFFF00 // Yellow (Warning)
        } else {
            0x00FF0000 // Red (Failure)
        };

        for y in 150..200 { 
            for x in 0..width {
                unsafe {
                    let offset = y * pitch + x;
                    if x < bar_width {
                        *video_ptr.add(offset) = usage_color;
                    } else {
                        *video_ptr.add(offset) = 0x00333333; // Dark Grey Background
                    }
                }
            }
        }

        // F. LIVE INTERACTION FEEDBACK
        // If the user types, print dots to prove the main loop isn't frozen.
        // We need to be careful not to spam the screen.
        let key_count = state::KEY_COUNT.load(Ordering::Relaxed);
        // "Animation" of a cursor at the bottom of the console area
        if frame_count % 60 == 0 {
            // Blink a cursor or something similar could go here
        }

        frame_count = frame_count.wrapping_add(1);
    }
}