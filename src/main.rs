#![feature(abi_x86_interrupt)]
#![feature(alloc_error_handler)]
#![no_std]
#![no_main]

// --- EXTERNAL CRATES ---
extern crate alloc;
use alloc::boxed::Box;
use alloc::vec::Vec;
use alloc::string::String;
use alloc::format;

use limine::request::FramebufferRequest;
use limine::BaseRevision;
use core::arch::x86_64::_rdtsc;
use core::sync::atomic::Ordering;

// --- MODULES ---
mod interrupts;
mod state;
mod writer;
mod allocator;
mod scheduler;
mod input; // <--- NEW: Keyboard Buffer
mod shell; // <--- NEW: Command Processor

#[used]
static BASE_REVISION: BaseRevision = BaseRevision::new();

#[used]
static FRAMEBUFFER_REQUEST: FramebufferRequest = FramebufferRequest::new();

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    writer::print("\n\n[KERNEL PANIC]\n");
    if let Some(location) = info.location() {
        writer::print("File: ");
        writer::print(location.file());
        writer::print("\nLine: ");
        writer::print("SEE SOURCE\n");
    }
    loop { core::hint::spin_loop(); }
}

// --- BACKGROUND TASKS ---
fn task_fast_math() {
    let mut x: u64 = 0;
    for i in 0..1000 {
        x = x.wrapping_add(i);
    }
    core::hint::black_box(x); 
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    // -----------------------------------------------------------------------
    // 1. SYSTEM BOOTSTRAP
    // -----------------------------------------------------------------------
    interrupts::init_idt();
    unsafe { interrupts::PICS.lock().initialize() };
    interrupts::enable_listening();
    x86_64::instructions::interrupts::enable();

    // -----------------------------------------------------------------------
    // 2. VIDEO INIT
    // -----------------------------------------------------------------------
    let framebuffer_response = FRAMEBUFFER_REQUEST.get_response().unwrap();
    let framebuffer = framebuffer_response.framebuffers().next().unwrap();
    
    let video_ptr = framebuffer.addr() as *mut u32;
    let width = framebuffer.width() as usize;
    let height = framebuffer.height() as usize;
    let pitch = framebuffer.pitch() as usize / 4;

    writer::Writer::init(video_ptr, width, height, pitch);
    
    // Clear screen once at startup
    if let Some(w) = writer::WRITER.lock().as_mut() {
        w.clear();
    }

    writer::print("Chronos OS v0.5 - Interactive Shell\n");
    writer::print("-----------------------------------\n");

    // -----------------------------------------------------------------------
    // 3. MEMORY INIT
    // -----------------------------------------------------------------------
    allocator::init_heap();
    writer::print("[ OK ] Heap Initialized\n");

    // -----------------------------------------------------------------------
    // 4. SCHEDULER INIT
    // -----------------------------------------------------------------------
    let mut chronos_scheduler = scheduler::Scheduler::new();

    // ADD TASKS:
    // 1. The Shell (Generous budget for typing responsiveness)
    chronos_scheduler.add_task("Shell", 100_000, shell::shell_task);

    // 2. Background System Check (Keep the scheduler busy)
    chronos_scheduler.add_task("SysCheck", 50_000, task_fast_math);

    writer::print("[ OK ] Scheduler Active.\n");
    writer::print("[INFO] Type 'help' for commands.\n\n");
    writer::print("> "); // The First Prompt

    // -----------------------------------------------------------------------
    // 5. THE MAIN LOOP
    // -----------------------------------------------------------------------
    loop {
        // NOTE: We REMOVED w.clear() here. 
        // We want the text history to stay on screen!

        // A. EXECUTE ALL TASKS (Including Shell)
        chronos_scheduler.execute_frame();

        // B. DRAW GLOBAL LOAD (Visual Fuel Gauge)
        // We move this to the BOTTOM of the screen so it doesn't overwrite text.
        let total_cost: u64 = chronos_scheduler.tasks.iter().map(|t| t.last_cost).sum();
        let global_budget = state::CYCLE_BUDGET.load(Ordering::Relaxed);
        
        let mut bar_width = ((total_cost as u128 * width as u128) / global_budget as u128) as usize;
        if bar_width > width { bar_width = width; }

        let color = if bar_width < width { 0x0000FF00 } else { 0x00FF0000 };
        
        // Draw bar at the very bottom (last 50 pixels)
        let bar_y_start = height - 50;
        for y in bar_y_start..height {
            for x in 0..width {
                unsafe {
                    let offset = y * pitch + x;
                    if x < bar_width {
                        *video_ptr.add(offset) = color;
                    } else {
                        *video_ptr.add(offset) = 0x00333333; // Dark Grey
                    }
                }
            }
        }

        // C. DELAY
        // We keep the loop tight for responsiveness, but a small delay helps stability
        for _ in 0..10_000 { core::hint::spin_loop(); }
    }
}