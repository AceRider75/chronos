#![feature(abi_x86_interrupt)]
#![feature(alloc_error_handler)] // Enable OOM handling
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
mod scheduler; // The new Time-Aware Scheduler

#[used]
static BASE_REVISION: BaseRevision = BaseRevision::new();

#[used]
static FRAMEBUFFER_REQUEST: FramebufferRequest = FramebufferRequest::new();

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    // Emergency printing if the kernel crashes
    writer::print("\n\n[KERNEL PANIC]\n");
    if let Some(location) = info.location() {
        writer::print("File: ");
        writer::print(location.file());
        writer::print("\nLine: ");
        // We can't format integers easily in panic without alloc, so just print a marker
        writer::print("SEE SOURCE\n");
    }
    loop { core::hint::spin_loop(); }
}

// --- DUMMY TASKS ---

// Task 1: A very fast task (Simple addition)
// This represents a system service like "Check Battery"
fn task_fast_math() {
    let mut x: u64 = 0;
    for i in 0..1000 {
        x = x.wrapping_add(i);
    }
    // 'black_box' prevents the compiler from deleting this loop during optimization
    core::hint::black_box(x); 
}

// Task 2: A heavy task (Simulated heavy load)
// This represents a user application like "Web Browser" or "Video Player"
// We intentionally make this heavy to test the Deadline Failure logic.
fn task_heavy_render() {
    let mut x: u64 = 0;
    // ADJUST THIS NUMBER if it always passes or always fails on your PC.
    // 5_000_000 is usually heavy enough for QEMU.
    for i in 0..100_000_000 { 
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
    
    // Clear screen initially
    if let Some(w) = writer::WRITER.lock().as_mut() {
        w.clear();
    }

    writer::print("Chronos OS v0.4\n");
    writer::print("--------------------------\n");

    // -----------------------------------------------------------------------
    // 3. MEMORY INIT
    // -----------------------------------------------------------------------
    allocator::init_heap();
    writer::print("[ OK ] Heap Initialized (100 KB)\n");

    // -----------------------------------------------------------------------
    // 4. SCHEDULER INIT
    // -----------------------------------------------------------------------
    let mut chronos_scheduler = scheduler::Scheduler::new();

    // Define the Contracts
    // "SysCheck" gets 50k cycles. If it takes longer, it FAILS.
    chronos_scheduler.add_task("SysCheck", 50_000, task_fast_math);

    // "RenderUI" gets 2M cycles. Our loop is designed to take ~5M cycles.
    // EXPECTATION: This task should print [ FAIL ].
    chronos_scheduler.add_task("RenderUI", 1, task_heavy_render);

    writer::print("[ OK ] Scheduler Initialized. Starting Main Loop...\n");

    // -----------------------------------------------------------------------
    // 5. THE MAIN LOOP
    // -----------------------------------------------------------------------
    loop {
        // A. Clear the screen (Dirty hack to update text)
        // In a real OS, we would only redraw what changed.
        if let Some(w) = writer::WRITER.lock().as_mut() {
             w.clear();
             // Manually reset cursor to top left so we overwrite previous text
             w.cursor_x = 10;
             w.cursor_y = 10;
        }

        writer::print("Chronos OS - Scheduler Active\n");
        writer::print("-----------------------------\n");

        // B. EXECUTE ALL TASKS
        chronos_scheduler.execute_frame();

        // C. DRAW DEBUG REPORT (Text)
        chronos_scheduler.draw_debug();

        // D. DRAW GLOBAL LOAD (Visual)
        // Calculate total time spent by all tasks
        let total_cost: u64 = chronos_scheduler.tasks.iter().map(|t| t.last_cost).sum();
        let global_budget = state::CYCLE_BUDGET.load(Ordering::Relaxed);
        
        let mut bar_width = ((total_cost as u128 * width as u128) / global_budget as u128) as usize;
        if bar_width > width { bar_width = width; }

        let color = if bar_width < width { 0x0000FF00 } else { 0x00FF0000 };
        
        // Draw bar at y=400
        for y in 400..450 {
            for x in 0..width {
                unsafe {
                    let offset = y * pitch + x;
                    if x < bar_width {
                        *video_ptr.add(offset) = color;
                    } else {
                        *video_ptr.add(offset) = 0x00333333;
                    }
                }
            }
        }

        // E. ARTIFICIAL DELAY
        // We slow down the loop so the text doesn't flicker too fast to read.
        // This simulates "Wait for V-Sync".
        for _ in 0..10_000_000 { core::hint::spin_loop(); }
    }
}