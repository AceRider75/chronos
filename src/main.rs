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
mod logger;
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
    // 1. HARDWARE INIT
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

    let hhdm_offset = HHDM_REQUEST.get_response().unwrap().offset();
    let memmap = MEMMAP_REQUEST.get_response().unwrap();
    let kernel_response = KERNEL_ADDR_REQUEST.get_response().unwrap();

    state::HHDM_OFFSET.store(hhdm_offset, Ordering::Relaxed);
    state::KERNEL_DELTA.store(kernel_response.virtual_base() - kernel_response.physical_base(), Ordering::Relaxed);

    unsafe { memory::init(hhdm_offset, memmap) };

    // 4. GUI INIT
    mouse::init(width, height);
    let mut desktop = compositor::Compositor::new(width, height);
    
    // FIX 1: Pass a string title instead of color
    let _ = compositor::Window::new(0, height - 30, width, 30, "Taskbar");

    writer::print("Chronos OS v0.97 (Window Manager)\n");
    writer::print("---------------------------------\n");
    
    // 5. SCHEDULER INIT
    let mut chronos_scheduler = scheduler::Scheduler::new();
    chronos_scheduler.add_task("Shell", 100_000, shell::shell_task);
    fn idle_task() { core::hint::black_box(0); }
    chronos_scheduler.add_task("Idle", 10_000, idle_task);

    writer::print("[INFO] Desktop Environment Loaded.\n");

    // --- GUI STATE ---
    let mut is_dragging = false;
    let mut drag_offset_x = 0;
    let mut drag_offset_y = 0;

    // 6. MAIN LOOP
    loop {
        let start = unsafe { core::arch::x86_64::_rdtsc() };
        chronos_scheduler.execute_frame();

        // A. GET INPUT
        let (mx, my, btn) = mouse::get_state();

        // B. WINDOW MANIPULATION
        if let Some(mut shell_mutex) = shell::SHELL.try_lock() {
            let win = &mut shell_mutex.window;

            if btn {
                if is_dragging {
                    // Apply Drag
                    if mx > drag_offset_x { win.x = mx - drag_offset_x; }
                    if my > drag_offset_y { win.y = my - drag_offset_y; }
                } else {
                    // Check for Title Bar Click
                    if win.is_title_bar(mx, my) {
                        is_dragging = true;
                        drag_offset_x = mx - win.x;
                        drag_offset_y = my - win.y;
                    }
                }
            } else {
                is_dragging = false;
            }

            // C. UPDATE TASKBAR CLOCK
            // FIX 2: Pass string title here too
            let mut taskbar = compositor::Window::new(0, height - 30, width, 30, "Taskbar");
            
            let time = time::read_rtc();
            use alloc::format;
            let time_str = format!("{:02}:{:02}:{:02}", time.hours, time.minutes, time.seconds);
            
            taskbar.cursor_x = width - 100;
            taskbar.cursor_y = 5; 
            taskbar.print(&time_str);

            // D. RENDER PASS
            // Draw order: Taskbar Background -> Shell Window -> Taskbar Clock Overlay
            // (Actually we just draw taskbar and shell. The taskbar var we just made has the clock)
            let windows = [&taskbar, win];
            desktop.render(&windows);
        }

        // E. FUEL GAUGE OVERLAY
        let end = unsafe { core::arch::x86_64::_rdtsc() };
        let elapsed = end - start;
        let cycle_budget = 20_000_000;
        let mut bar_width = ((elapsed as u128 * width as u128) / cycle_budget as u128) as usize;
        if bar_width > width { bar_width = width; }

        let color = if bar_width < width { 0x0000FF00 } else { 0x00FF0000 };
        let bar_y_start = height - 5; // Very thin line at bottom

        for y in bar_y_start..height {
            for x in 0..width {
                unsafe {
                    let offset = y * pitch + x;
                    if x < bar_width { *video_ptr.add(offset) = color; }
                }
            }
        }

        // F. STABILITY DELAY
        // for _ in 0..50_000 { core::hint::spin_loop(); }
    }
}