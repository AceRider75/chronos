#![feature(abi_x86_interrupt)]
#![feature(alloc_error_handler)]
#![no_std]
#![no_main]

extern crate alloc;

use limine::request::FramebufferRequest;
use limine::BaseRevision;

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
        writer::print("\n");
    }
    loop { core::hint::spin_loop(); }
}

fn user_mode_app() -> ! {
    loop { core::hint::spin_loop(); }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    // 1. GDT & TSS (Must be first)
    gdt::init(); 

    // 2. Interrupts
    interrupts::init_idt();
    unsafe { interrupts::PICS.lock().initialize() };
    interrupts::enable_listening();
    
    // NOTE: We do NOT enable CPU interrupts (sti) yet.
    // We want the jump to be clean without timer noise.

    // 3. Video
    let framebuffer_response = FRAMEBUFFER_REQUEST.get_response().unwrap();
    let framebuffer = framebuffer_response.framebuffers().next().unwrap();
    let video_ptr = framebuffer.addr() as *mut u32;
    let width = framebuffer.width() as usize;
    let height = framebuffer.height() as usize;
    let pitch = framebuffer.pitch() as usize / 4;

    writer::Writer::init(video_ptr, width, height, pitch);
    if let Some(w) = writer::WRITER.lock().as_mut() { w.clear(); }

    allocator::init_heap();

    // 4. Status
    writer::print("Chronos OS v0.8 - Phase 8 Test\n");
    writer::print("------------------------------\n");
    writer::print("[ OK ] GDT, TSS, & Heap Ready\n");
    writer::print("[INFO] Jumping to Ring 3 (User Mode)...\n");

    // 5. The Transition
    // We get the magic numbers (selectors) that tell the CPU "Be a User"
    let (user_code, user_data) = gdt::get_user_selectors();

    // We call the assembly function.
    // EXPECTATION: 
    // 1. CPU Switches to Ring 3.
    // 2. CPU tries to execute 'user_mode_app'.
    // 3. CPU sees 'user_mode_app' is in Kernel Memory.
    // 4. CPU triggers Page Fault (Vector 14).
    // 5. Our 'page_fault_handler' prints SUCCESS.
    userspace::jump_to_code(user_mode_app, user_code, user_data);

    // Unreachable
    loop {}
}