#![feature(abi_x86_interrupt)]
#![no_std]
#![no_main]

use limine::request::FramebufferRequest;
use limine::BaseRevision;
use core::arch::x86_64::_rdtsc;
use core::sync::atomic::Ordering;

mod interrupts;
mod state;

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
    // 1. INITIALIZE NERVOUS SYSTEM
    interrupts::init_idt();
    unsafe { interrupts::PICS.lock().initialize() };
    
    // FORCE UNMASK: Explicitly tell PIC to listen to Timer & Keyboard
    interrupts::enable_listening();
    
    // UNMUTE CPU
    x86_64::instructions::interrupts::enable();

    let framebuffer_response = FRAMEBUFFER_REQUEST.get_response().unwrap();
    let framebuffer = framebuffer_response.framebuffers().next().unwrap();
    
    let video_ptr = framebuffer.addr() as *mut u32;
    let width = framebuffer.width() as usize;
    let height = framebuffer.height() as usize;
    let pitch = framebuffer.pitch() as usize / 4;

    let mut frame_count: u64 = 0;
    
    loop {
        let cycle_budget = state::CYCLE_BUDGET.load(Ordering::Relaxed);
        let start_time = unsafe { _rdtsc() };

        // ---------------------------------------------------------
        // DEBUG LATCH
        // ---------------------------------------------------------
        // Start RED. If we hear ANYTHING from the hardware, turn GREEN.
        let key_count = state::KEY_COUNT.load(Ordering::Relaxed);
        
        // If count > 0, interrupts are alive.
        let debug_color = if key_count > 0 { 
            0x0000FF00 // GREEN (ALIVE)
        } else { 
            0x00FF0000 // RED (DEAD)
        };

        for y in 0..50 {
            for x in 0..50 {
                unsafe {
                    *video_ptr.add(y * pitch + x) = debug_color;
                }
            }
        }

        // ---------------------------------------------------------
        // BACKGROUND & GAUGE (Standard)
        // ---------------------------------------------------------
        let blue_val = (frame_count % 255) as u32;
        let bg_color = 0x00102000 | blue_val; 
        for y in 50..height { 
            for x in 0..width {
                unsafe { *video_ptr.add(y * pitch + x) = bg_color; }
            }
        }

        let end_time = unsafe { _rdtsc() };
        let elapsed = end_time - start_time;
        let mut bar_width = ((elapsed as u128 * width as u128) / cycle_budget as u128) as usize;
        if bar_width > width { bar_width = width; }
        let usage_color = if bar_width < width / 2 { 0x0000FF00 } else { 0x00FF0000 };

        for y in 0..50 { 
            for x in 50..width {
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