#![feature(abi_x86_interrupt)]
#![feature(alloc_error_handler)]
#![no_std]
#![no_main]

// --- EXTERNAL CRATES ---
extern crate alloc;

use limine::request::{FramebufferRequest, HhdmRequest}; // Added HhdmRequest
use limine::BaseRevision;

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
mod memory; // The VMM Module

#[used]
static BASE_REVISION: BaseRevision = BaseRevision::new();

#[used]
static FRAMEBUFFER_REQUEST: FramebufferRequest = FramebufferRequest::new();

// Request the Higher Half Direct Map to edit Page Tables
#[used]
static HHDM_REQUEST: HhdmRequest = HhdmRequest::new();

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

// -----------------------------------------------------------------------
// USER MODE APPLICATION (Ring 3)
// -----------------------------------------------------------------------
fn user_mode_app() -> ! {
    // If the OS freezes here, it means we are successfully running in Ring 3!
    loop {
        core::hint::spin_loop();
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    // -----------------------------------------------------------------------
    // 1. SYSTEM BOOTSTRAP
    // -----------------------------------------------------------------------
    gdt::init(); 
    interrupts::init_idt();
    unsafe { interrupts::PICS.lock().initialize() };
    interrupts::enable_listening();
    // Note: Interrupts are NOT enabled yet (cli) to keep the jump clean.

    // -----------------------------------------------------------------------
    // 2. VIDEO & MEMORY INIT
    // -----------------------------------------------------------------------
    let framebuffer_response = FRAMEBUFFER_REQUEST.get_response().unwrap();
    let framebuffer = framebuffer_response.framebuffers().next().unwrap();
    let video_ptr = framebuffer.addr() as *mut u32;
    let width = framebuffer.width() as usize;
    let height = framebuffer.height() as usize;
    let pitch = framebuffer.pitch() as usize / 4;

    writer::Writer::init(video_ptr, width, height, pitch);
    if let Some(w) = writer::WRITER.lock().as_mut() { w.clear(); }

    allocator::init_heap();

    writer::print("Chronos OS v0.9 - Memory Protection\n");
    writer::print("-----------------------------------\n");
    writer::print("[ OK ] Kernel Services Initialized\n");

    // -----------------------------------------------------------------------
    // 3. VIRTUAL MEMORY MANAGER (VMM) SETUP
    // -----------------------------------------------------------------------
    // We need to tell the CPU: "Let Ring 3 touch the user_mode_app address"
    
    let hhdm_response = HHDM_REQUEST.get_response().unwrap();
    let hhdm_offset = hhdm_response.offset();
    
    // Initialize the Page Table Mapper
    let mut mapper = unsafe { memory::init(hhdm_offset) };
    
    // Calculate address of our app function
    let app_addr = user_mode_app as usize as u64;
    
    writer::print("[INFO] Unlocking Memory Page for User Mode...\n");
    
    // Unlock the specific page where the code lives.
    // We check the next page too just in case the function crosses a boundary.
    memory::mark_as_user(&mut mapper, app_addr);
    memory::mark_as_user(&mut mapper, app_addr + 4096);

    writer::print("[ OK ] Page Tables Updated (USER_ACCESSIBLE flag set)\n");
    writer::print("[INFO] Jumping to Ring 3...\n");

    // -----------------------------------------------------------------------
    // 4. THE JUMP
    // -----------------------------------------------------------------------
    let (user_code, user_data) = gdt::get_user_selectors();

    // Call assembly jump.
    // SUCCESS: Screen freezes at "Jumping to Ring 3..." (Infinite loop in app)
    // FAILURE: Page Fault Panic (if unlocking failed)
    userspace::jump_to_code(user_mode_app, user_code, user_data);

    loop {}
}