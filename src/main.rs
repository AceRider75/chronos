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
mod time;

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
    // FIX: Use writer::print instead of writer::print
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

    let hhdm_offset = HHDM_REQUEST.get_response().unwrap().offset();
    let memmap = MEMMAP_REQUEST.get_response().unwrap();
    let kernel_response = KERNEL_ADDR_REQUEST.get_response().unwrap();

    state::HHDM_OFFSET.store(hhdm_offset, Ordering::Relaxed);
    state::KERNEL_DELTA.store(kernel_response.virtual_base() - kernel_response.physical_base(), Ordering::Relaxed);

    unsafe { memory::init(hhdm_offset, memmap) };

    // -----------------------------------------------------------------------
    // 4. GUI & COMPOSITOR INIT
    // -----------------------------------------------------------------------
    mouse::init(width, height);
    
    // Create the Compositor
    let mut desktop = compositor::Compositor::new(width, height);
    
    // Create a decorative background window (Taskbar)
    let bar = compositor::Window::new(0, height - 40, width, 40, 0xFF333333);

    // FIX: Use writer::print instead of writer::print
    writer::print("Chronos OS v0.96 (GUI Enabled)\n");
    writer::print("--------------------------------\n");
    writer::print("[ OK ] HAL & Protection Initialized\n");
    writer::print("[ OK ] VMM & Physical Memory Manager Online\n");
    writer::print("[ OK ] Filesystem & Network Stack Ready\n");
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

    writer::print("[INFO] Entering Interactive Mode.\n");
    let mut is_dragging = false;
    let mut drag_offset_x = 0;
    let mut drag_offset_y = 0;
    // -----------------------------------------------------------------------
    // 6. MAIN TIME-AWARE LOOP
    // -----------------------------------------------------------------------
    loop {
        let start = unsafe { core::arch::x86_64::_rdtsc() };

        // Run the cooperative tasks
        chronos_scheduler.execute_frame();
        let (mx, my, btn) = mouse::get_state();

        let end = unsafe { core::arch::x86_64::_rdtsc() };
        let elapsed = end - start;

        // --- RENDER PASS ---
        // Lock the Shell to get access to its terminal window
        if let Some(mut shell_mutex) = shell::SHELL.try_lock() {
            let win = &mut shell_mutex.window;

            if btn {
                // Mouse is DOWN
                if is_dragging {
                    // CONTINUE DRAGGING
                    // We prevent underflow by checking if mouse > offset
                    if mx > drag_offset_x { win.x = mx - drag_offset_x; }
                    if my > drag_offset_y { win.y = my - drag_offset_y; }
                } else {
                    // CLICK START?
                    // Check if mouse is inside the window
                    if win.contains(mx, my) {
                        is_dragging = true;
                        // Calculate offset so window doesn't "snap"
                        drag_offset_x = mx - win.x;
                        drag_offset_y = my - win.y;
                    }
                }
            } else {
                // Mouse is UP
                is_dragging = false;
            }

            // 3. RENDER PASS
            // We force the taskbar to stay at bottom, but shell can move
            // (Note: Taskbar creates a new struct every frame here, which is fine for now)
            let current_bar = compositor::Window::new(0, height - 30, width, 30, 0xFF202020);
            let mut taskbar = compositor::Window::new(0, height - 30, width, 30, 0xFF202020);
            let time = time::read_rtc();
            use alloc::format;
            let time_str = format!("{:02}:{:02}:{:02}", time.hours, time.minutes, time.seconds);
            
            // Position cursor at bottom right
            taskbar.cursor_x = width - 100;
            taskbar.cursor_y = 5; 
            taskbar.print(&time_str); // Draw time onto taskbar            
            let windows = [&current_bar, win];
            let windows = [&taskbar, win];
            desktop.render(&windows);
        }

        // --- FUEL GAUGE ---
        // We draw this *directly* to video memory (Overlay)
        let cycle_budget = 2_500_000;
        let mut bar_width = ((elapsed as u128 * width as u128) / cycle_budget as u128) as usize;
        if bar_width > width { bar_width = width; }

        let color = if bar_width < width { 0x0000FF00 } else { 0x00FF0000 };
        let bar_y_start = height - 10;

        for y in bar_y_start..height {
            for x in 0..width {
                unsafe {
                    let offset = y * pitch + x;
                    if x < bar_width {
                        *video_ptr.add(offset) = color;
                    } else {
                        // Don't draw background here, let compositor handle it
                    }
                }
            }
        }

        // Stability delay
        // for _ in 0..50_000 { core::hint::spin_loop(); }
    }
}