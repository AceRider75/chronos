#![feature(abi_x86_interrupt)]
#![feature(alloc_error_handler)]
#![no_std]
#![no_main]

// --- EXTERNAL CRATES ---
extern crate alloc;

use limine::request::{FramebufferRequest, HhdmRequest};
use limine::BaseRevision;
use core::sync::atomic::Ordering;

// --- MODULES ---
mod interrupts;
mod state;
mod writer;
mod allocator;
mod scheduler;
mod input;
mod shell;
mod fs;
mod gdt;
mod userspace;
mod memory;

// --- LIMINE REQUESTS ---
#[used]
static BASE_REVISION: BaseRevision = BaseRevision::new();

#[used]
static FRAMEBUFFER_REQUEST: FramebufferRequest = FramebufferRequest::new();

// Only ONE HhdmRequest in the entire program!
#[used]
static HHDM_REQUEST: HhdmRequest = HhdmRequest::new();

// --- PANIC HANDLER ---
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    writer::print("\n\n[KERNEL PANIC]\n");
    if let Some(location) = info.location() {
        writer::print("File: ");
        writer::print(location.file());
        writer::print("\n");
    }
    loop { core::hint::spin_loop(); }
}

// --- KERNEL ENTRY ---
#[no_mangle]
pub extern "C" fn _start() -> ! {
    // 1. SYSTEM BOOTSTRAP
    gdt::init(); 
    interrupts::init_idt();
    unsafe { interrupts::PICS.lock().initialize() };
    interrupts::enable_listening();
    x86_64::instructions::interrupts::enable(); 

    // 2. VIDEO INIT
    let framebuffer_response = FRAMEBUFFER_REQUEST.get_response().unwrap();
    let framebuffer = framebuffer_response.framebuffers().next().unwrap();
    let video_ptr = framebuffer.addr() as *mut u32;
    let width = framebuffer.width() as usize;
    let height = framebuffer.height() as usize;
    let pitch = framebuffer.pitch() as usize / 4;

    writer::Writer::init(video_ptr, width, height, pitch);
    if let Some(w) = writer::WRITER.lock().as_mut() { w.clear(); }

    // 3. MEMORY INIT
    allocator::init_heap();

    // 4. VMM CONFIGURATION
    // Get the offset from Limine and save it to Global State
    let hhdm_response = HHDM_REQUEST.get_response().unwrap();
    let hhdm_offset = hhdm_response.offset();
    state::HHDM_OFFSET.store(hhdm_offset, Ordering::Relaxed);
    
    // Initialize the mapper (just to verify it works)
    unsafe { memory::init(hhdm_offset) };

    // 5. WELCOME LOGS
    writer::print("Chronos OS v0.9\n");
    writer::print("--------------------------------\n");
    writer::print("[ OK ] Hardware Initialized\n");
    writer::print("[ OK ] File System Mounted\n");
    writer::print("[INFO] Scheduler Initialized.\n\n");
    
    // 6. SCHEDULER SETUP
    let mut chronos_scheduler = scheduler::Scheduler::new();

    // Add Shell (User Interface)
    chronos_scheduler.add_task("Shell", 100_000, shell::shell_task);

    // Add Background Idle Task
    fn idle_task() { core::hint::black_box(0); }
    chronos_scheduler.add_task("Idle", 10_000, idle_task);

    writer::print("> "); // Initial Prompt

    // 7. MAIN LOOP
    loop {
        let start = unsafe { core::arch::x86_64::_rdtsc() };

        // Run Tasks
        chronos_scheduler.execute_frame();

        let end = unsafe { core::arch::x86_64::_rdtsc() };
        let elapsed = end - start;

        // Draw Fuel Gauge (Bottom 10 pixels)
        let cycle_budget = 2_500_000;
        let mut bar_width = ((elapsed as u128 * width as u128) / cycle_budget as u128) as usize;
        if bar_width > width { bar_width = width; }

        let color = if bar_width < width { 0x0000FF00 } else { 0x00FF0000 };
        let bar_y_start = height - 10;

        for y in bar_y_start..height {
            for x in 0..width {
                unsafe {
                    let offset = y * pitch + x;
                    if x < bar_width { *video_ptr.add(offset) = color; } 
                    else { *video_ptr.add(offset) = 0x00333333; }
                }
            }
        }

        // Delay to stabilize frame rate
        for _ in 0..50_000 { core::hint::spin_loop(); }
    }
}