#![feature(abi_x86_interrupt)]
#![feature(alloc_error_handler)]
#![no_std]
#![no_main]

// --- EXTERNAL CRATES ---
extern crate alloc;

use limine::request::{FramebufferRequest, HhdmRequest, KernelAddressRequest}; // Added KernelAddressRequest
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

// --- LIMINE REQUESTS ---
#[used]
static BASE_REVISION: BaseRevision = BaseRevision::new();

#[used]
static FRAMEBUFFER_REQUEST: FramebufferRequest = FramebufferRequest::new();

#[used]
static HHDM_REQUEST: HhdmRequest = HhdmRequest::new();

// NEW: We need this to find where the Kernel is in Physical RAM
#[used]
static KERNEL_ADDR_REQUEST: KernelAddressRequest = KernelAddressRequest::new();

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
    // -----------------------------------------------------------------------
    // 1. SYSTEM BOOTSTRAP
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
    let framebuffer = framebuffer_response.framebuffers().next().unwrap();
    let video_ptr = framebuffer.addr() as *mut u32;
    let width = framebuffer.width() as usize;
    let height = framebuffer.height() as usize;
    let pitch = framebuffer.pitch() as usize / 4;

    writer::Writer::init(video_ptr, width, height, pitch);
    if let Some(w) = writer::WRITER.lock().as_mut() { w.clear(); }

    // -----------------------------------------------------------------------
    // 3. MEMORY INIT
    // -----------------------------------------------------------------------
    allocator::init_heap();

    // -----------------------------------------------------------------------
    // 4. MEMORY MAPPING CONFIGURATION (Critical for DMA/VMM)
    // -----------------------------------------------------------------------
    
    // A. Handle HHDM (Higher Half Direct Map)
    let hhdm_response = HHDM_REQUEST.get_response().unwrap();
    let hhdm_offset = hhdm_response.offset();
    state::HHDM_OFFSET.store(hhdm_offset, Ordering::Relaxed);
    
    // B. Handle Kernel Physical Address (NEW FIX)
    // The heap is inside the kernel binary (.bss section).
    // To give the Network Card access, we need: Phys = Virt - Delta.
    let kernel_response = KERNEL_ADDR_REQUEST.get_response().unwrap();
    let k_virt = kernel_response.virtual_base();
    let k_phys = kernel_response.physical_base();
    let k_delta = k_virt - k_phys;
    state::KERNEL_DELTA.store(k_delta, Ordering::Relaxed);

    // Initialize VMM
    unsafe { memory::init(hhdm_offset) };

    // -----------------------------------------------------------------------
    // 5. WELCOME LOGS
    // -----------------------------------------------------------------------
    writer::print("Chronos OS v0.95 - Network Enabled\n");
    writer::print("----------------------------------\n");
    writer::print("[ OK ] Hardware Initialized\n");
    writer::print("[ OK ] Memory Map Calculated\n");
    writer::print("[ OK ] File System Mounted\n");
    
    // -----------------------------------------------------------------------
    // 6. SCHEDULER SETUP
    // -----------------------------------------------------------------------
    let mut chronos_scheduler = scheduler::Scheduler::new();

    chronos_scheduler.add_task("Shell", 100_000, shell::shell_task);

    fn idle_task() { core::hint::black_box(0); }
    chronos_scheduler.add_task("Idle", 10_000, idle_task);

    writer::print("[ OK ] Scheduler Active.\n\n");
    writer::print("> "); 

    // -----------------------------------------------------------------------
    // 7. MAIN LOOP
    // -----------------------------------------------------------------------
    loop {
        let start = unsafe { core::arch::x86_64::_rdtsc() };

        chronos_scheduler.execute_frame();

        let end = unsafe { core::arch::x86_64::_rdtsc() };
        let elapsed = end - start;

        // Visual Fuel Gauge
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

        for _ in 0..50_000 { core::hint::spin_loop(); }
    }
}