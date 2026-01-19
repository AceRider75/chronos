#![feature(abi_x86_interrupt)]
#![feature(alloc_error_handler)] // <--- ADD THIS FEATURE
#![no_std]
#![no_main]

// NEW: Import the alloc library
extern crate alloc;
use alloc::boxed::Box;
use alloc::vec::Vec;
use alloc::string::String;
use alloc::format;

use limine::request::FramebufferRequest;
use limine::BaseRevision;
use core::arch::x86_64::_rdtsc;
use core::sync::atomic::Ordering;

mod interrupts;
mod state;
mod writer;
mod allocator; // <--- Import the new module

#[used]
static BASE_REVISION: BaseRevision = BaseRevision::new();

#[used]
static FRAMEBUFFER_REQUEST: FramebufferRequest = FramebufferRequest::new();

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    // A simple panic handler that prints the error to the screen
    writer::print("\n\n[KERNEL PANIC]\n");
    if let Some(location) = info.location() {
        // We can't format strings easily in panic yet without alloc, 
        // but we can try basic printing
        writer::print("Location: ");
        writer::print(location.file());
        writer::print("\n");
    }
    loop {}
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    interrupts::init_idt();
    unsafe { interrupts::PICS.lock().initialize() };
    interrupts::enable_listening();
    x86_64::instructions::interrupts::enable();

    let framebuffer_response = FRAMEBUFFER_REQUEST.get_response().unwrap();
    let framebuffer = framebuffer_response.framebuffers().next().unwrap();
    
    let video_ptr = framebuffer.addr() as *mut u32;
    let width = framebuffer.width() as usize;
    let height = framebuffer.height() as usize;
    let pitch = framebuffer.pitch() as usize / 4;

    writer::Writer::init(video_ptr, width, height, pitch);
    if let Some(w) = writer::WRITER.lock().as_mut() {
        w.clear();
    }

    writer::print("Chronos OS v0.3\n");
    writer::print("--------------------------\n");

    // 1. INITIALIZE HEAP
    allocator::init_heap();
    writer::print("[ OK ] Heap Initialized (100 KB)\n");

    // 2. TEST ALLOCATION (Box)
    let heap_value = Box::new(42);
    writer::print("[ OK ] Box Allocated: ");
    if *heap_value == 42 {
        writer::print("Success\n");
    } else {
        writer::print("Failed\n");
    }

    // 3. TEST DYNAMIC VECTOR
    // This proves we can grow memory dynamically
    let mut vec = Vec::new();
    for i in 0..5 {
        vec.push(i);
    }
    writer::print("[ OK ] Vec Allocated\n");

    // 4. TEST STRING FORMATTING
    // We can now use format! macro because we have a heap!
    let status_msg = format!("[INFO] Heap Test Complete. Vector Size: {}\n", vec.len());
    writer::print(&status_msg);

    let mut frame_count: u64 = 0;
    
    loop {
        let cycle_budget = state::CYCLE_BUDGET.load(Ordering::Relaxed);
        let start_time = unsafe { _rdtsc() };
        let _key_count = state::KEY_COUNT.load(Ordering::Relaxed);

        // Background
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

        let end_time = unsafe { _rdtsc() };
        let elapsed = end_time - start_time;

        // Fuel Gauge
        let mut bar_width = ((elapsed as u128 * width as u128) / cycle_budget as u128) as usize;
        if bar_width > width { bar_width = width; }

        let usage_color = if bar_width < width / 2 { 
            0x0000FF00 
        } else if bar_width < width {
            0x00FFFF00 
        } else {
            0x00FF0000 
        };

        for y in 150..200 { 
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