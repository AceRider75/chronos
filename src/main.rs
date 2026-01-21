#![feature(abi_x86_interrupt)]
#![feature(alloc_error_handler)]
#![no_std]
#![no_main]

// --- EXTERNAL CRATES ---
extern crate alloc;

use limine::request::{FramebufferRequest, HhdmRequest, ExecutableAddressRequest, MemoryMapRequest}; 
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
mod pci;
mod rtl8139;
mod net;
mod elf;
mod mouse;
mod compositor;

// --- LIMINE BOOTLOADER REQUESTS ---
#[used]
static BASE_REVISION: BaseRevision = BaseRevision::new();

#[used]
static FRAMEBUFFER_REQUEST: FramebufferRequest = FramebufferRequest::new();

#[used]
static HHDM_REQUEST: HhdmRequest = HhdmRequest::new();

#[used]
static KERNEL_ADDR_REQUEST: ExecutableAddressRequest = ExecutableAddressRequest::new();

#[used]
static MEMMAP_REQUEST: MemoryMapRequest = MemoryMapRequest::new();

// --- PANIC HANDLER ---
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    writer::print("\n\n!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!\n");
    writer::print("[KERNEL PANIC]\n");
    if let Some(location) = info.location() {
        writer::print("Source: ");
        writer::print(location.file());
        writer::print("\n");
    }
    writer::print("!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!\n");
    loop { core::hint::spin_loop(); }
}

// --- KERNEL ENTRY POINT ---
#[no_mangle]
pub extern "C" fn _start() -> ! {
    // -----------------------------------------------------------------------
    // 1. HARDWARE ABSTRACTION LAYER (HAL) INIT
    // -----------------------------------------------------------------------
    gdt::init(); 
    interrupts::init_idt();
    unsafe { interrupts::PICS.lock().initialize() };
    interrupts::enable_listening();
    x86_64::instructions::interrupts::enable(); 

    // -----------------------------------------------------------------------
    // 2. VIDEO INIT
    // -----------------------------------------------------------------------
    let framebuffer_response = FRAMEBUFFER_REQUEST.get_response().unwrap();
    let fb = framebuffer_response.framebuffers().next().unwrap();

    let video_ptr = fb.addr() as *mut u32;
    let width = fb.width() as usize;
    let height = fb.height() as usize;
    let pitch = fb.pitch() as usize / 4;

    writer::Writer::init(video_ptr, width, height, pitch);
    
    
    
    if let Some(w) = writer::WRITER.lock().as_mut() {
        w.clear();
    }



    // -----------------------------------------------------------------------
    // 3. MEMORY & VMM INIT
    // -----------------------------------------------------------------------
    allocator::init_heap();

    // Get memory information from Limine
    let hhdm_offset = HHDM_REQUEST.get_response().unwrap().offset();
    let memmap = MEMMAP_REQUEST.get_response().unwrap();
    let kernel_response = KERNEL_ADDR_REQUEST.get_response().unwrap();

    // Store global offsets for Driver/Shell use
    state::HHDM_OFFSET.store(hhdm_offset, Ordering::Relaxed);
    state::KERNEL_DELTA.store(kernel_response.virtual_base() - kernel_response.physical_base(), Ordering::Relaxed);

    // Initialize Virtual Memory Manager with the Memory Map
    // This allows the OS to allocate physical RAM to build new page tables.
    unsafe { memory::init(hhdm_offset, memmap) };

    // -----------------------------------------------------------------------
    // 4. STATUS REPORT
    // -----------------------------------------------------------------------
    writer::print("Chronos OS v0.95 (Build: Era 2)\n");
    writer::print("----------------------------------------\n");
    writer::print("[ OK ] HAL & Protection Initialized\n");
    writer::print("[ OK ] VMM & Physical Memory Manager Online\n");
    writer::print("[ OK ] Filesystem & Network Stack Ready\n");
    mouse::init(width, height);
    let mut desktop = compositor::Compositor::new(width, height);
    let win1 = compositor::Window::new(100, 100, 300, 200, 0xFF880000);
    desktop.add_window(win1);
    let win2 = compositor::Window::new(200, 200, 200, 200, 0xFF008800);
    desktop.add_window(win2);
    writer::print("[ OK ] Compositor Initialized\n");
    
    // -----------------------------------------------------------------------
    // 5. SCHEDULER SETUP
    // -----------------------------------------------------------------------
    let mut chronos_scheduler = scheduler::Scheduler::new();

    // Shell Task (Priority)
    chronos_scheduler.add_task("Shell", 100_000, shell::shell_task);

    // Idle Task (Background)
    fn idle_task() { core::hint::black_box(0); }
    chronos_scheduler.add_task("Idle", 10_000, idle_task);

    writer::print("[INFO] Entering Interactive Mode.\n\n");
    writer::print("> "); 

    // -----------------------------------------------------------------------
    // 6. MAIN TIME-AWARE LOOP
    // -----------------------------------------------------------------------
    loop {
        let start = unsafe { core::arch::x86_64::_rdtsc() };

        // Run the cooperative tasks
        chronos_scheduler.execute_frame();

        let end = unsafe { core::arch::x86_64::_rdtsc() };
        let elapsed = end - start;

        // Visual Fuel Gauge (Last 10 pixels of the screen)
        let cycle_budget = 2_500_000;
        let mut bar_width = ((elapsed as u128 * width as u128) / cycle_budget as u128) as usize;
        if bar_width > width { bar_width = width; }

        let color = if bar_width < width { 0x0000FF00 } else { 0x00FF0000 };
        let bar_y_start = height - 10;
        desktop.render();

        for y in bar_y_start..height {
            for x in 0..width {
                unsafe {
                    let offset = y * pitch + x;
                    if x < bar_width {
                        *video_ptr.add(offset) = color;
                    } else {
                        *video_ptr.add(offset) = 0x00151515; // Subtle background
                    }
                }
            }
        }

        // Stability delay
        for _ in 0..50_000 { core::hint::spin_loop(); }
    }
}