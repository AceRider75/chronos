#![feature(abi_x86_interrupt)]
#![feature(alloc_error_handler)]
#![feature(naked_functions)]
#![no_std]
#![no_main]

extern crate alloc;

use limine::request::{FramebufferRequest, HhdmRequest, ExecutableAddressRequest, MemoryMapRequest}; 
use limine::BaseRevision;
use core::sync::atomic::Ordering;

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
mod serial; // NEW
mod ata;
mod fat;

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

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    writer::print("\n\n!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!\n");
    writer::print("[KERNEL PANIC] SYSTEM HALTED\n");
    
    // FIX: Just use the message directly, it's not an Option anymore
    use alloc::format;
    writer::print(&format!("Error: {}\n", info.message()));

    if let Some(location) = info.location() {
        writer::print("File: ");
        writer::print(location.file());
        
        // We can now format the line number too!
        writer::print(&format!("\nLine: {}", location.line()));
    } else {
        writer::print("\nUnknown Location");
    }
    
    writer::print("\n!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!\n");
    loop { core::hint::spin_loop(); }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    // 1. HARDWARE INIT
    gdt::init(); 
    interrupts::init_idt();
    unsafe { interrupts::PICS.lock().initialize() };
    interrupts::init_pit();
    interrupts::enable_listening();
    x86_64::instructions::interrupts::enable(); 

    // 2. VIDEO INIT
    let framebuffer_response = FRAMEBUFFER_REQUEST.get_response().unwrap();
    let fb = framebuffer_response.framebuffers().next().unwrap();
    let video_ptr = fb.addr() as *mut u32;
    let width = fb.width() as usize;
    let height = fb.height() as usize;
    let pitch = fb.pitch() as usize / 4;

    // SAVE VIDEO STATE
    state::VIDEO_PTR.store(video_ptr as u64, Ordering::Relaxed);
    state::SCREEN_WIDTH.store(width, Ordering::Relaxed);
    state::SCREEN_HEIGHT.store(height, Ordering::Relaxed);

    writer::Writer::init(video_ptr, width, height, pitch);
    if let Some(w) = writer::WRITER.lock().as_mut() { w.clear(); }

    allocator::init_heap();

    // 3. MEMORY INIT
    let hhdm_offset = HHDM_REQUEST.get_response().unwrap().offset();
    let memmap = MEMMAP_REQUEST.get_response().unwrap();
    let kernel_response = KERNEL_ADDR_REQUEST.get_response().unwrap();

    state::HHDM_OFFSET.store(hhdm_offset, Ordering::Relaxed);
    state::KERNEL_DELTA.store(kernel_response.virtual_base() - kernel_response.physical_base(), Ordering::Relaxed);

    unsafe { memory::init(hhdm_offset, memmap) };
    fs::init();

    // 4. GUI INIT
    mouse::init(width, height);
    let mut desktop = compositor::Compositor::new(width, height);
    
    // 5. SCHEDULER SETUP (GLOBAL)
    // We use a block {} to lock, add tasks, and then release the lock immediately
    {
        let mut sched = scheduler::SCHEDULER.lock();
        sched.add_task("Shell", 10_000_000, shell::shell_task, 0);
        
        extern "C" fn idle_task(_arg: u64) { core::hint::black_box(0); }
        sched.add_task("Idle", 10_000, idle_task, 0);
        

    }

    writer::print("Chronos OS v0.98 (System Monitor)\n");
    writer::print("[INFO] Entering Interactive Mode.\n");

    let mut is_dragging = false;
    let mut drag_offset_x = 0;
    let mut drag_offset_y = 0;

    // 6. MAIN LOOP
    const FRAME_BUDGET_CYCLES: u64 = 50_000_000;

    loop {
        let start = unsafe { core::arch::x86_64::_rdtsc() };

        // Run scheduler step (handles context switching)
        scheduler::step();

        // --- GUI LOGIC ---
        let (mx, my, btn) = mouse::get_state();

        // 1. Taskbar (Always available)
        let mut taskbar = compositor::Window::new(0, height - 30, width, 30, "Taskbar");
        let time = time::read_rtc();
        use alloc::format;
        let time_str = format!("{:02}:{:02}:{:02}", time.hours, time.minutes, time.seconds);
        taskbar.cursor_x = width - 100;
        taskbar.cursor_y = 5;
        taskbar.print(&time_str);

        // 2. Try to render Shell Windows (Non-blocking to avoid deadlock with preempted Shell task)
        if let Some(mut shell_lock) = shell::SHELL.try_lock() {
            if let Some(ref mut shell_mutex) = *shell_lock {
                // ... GUI Logic ...
                // A. Update Mouse/Focus
                let mut is_dragging_local = is_dragging;
                let mut drag_offset_x_local = drag_offset_x; // local copy
                let mut drag_offset_y_local = drag_offset_y;

                 // A. Focus / Z-Order
                if btn && !is_dragging_local {
                    let mut clicked_idx = None;
                    for (i, win) in shell_mutex.windows.iter().enumerate().rev() {
                        if win.contains(mx, my) {
                            clicked_idx = Some(i);
                            break;
                        }
                    }
                    if let Some(idx) = clicked_idx {
                        let win = shell_mutex.windows.remove(idx);
                        shell_mutex.windows.push(win);
                        let new_idx = shell_mutex.windows.len() - 1;
                        shell_mutex.active_idx = new_idx;
                        
                        let win = &mut shell_mutex.windows[new_idx];
                        let action = win.handle_title_bar_click(mx, my);

                        if action == 1 {
                             if shell_mutex.windows.len() > 1 {
                                 shell_mutex.windows.remove(new_idx);
                                 if shell_mutex.active_idx >= shell_mutex.windows.len() {
                                     shell_mutex.active_idx = if shell_mutex.windows.is_empty() { 0 } else { shell_mutex.windows.len() - 1 };
                                 }
                                 writer::print("Window Closed via X Button\n");
                             } else {
                                  // writer::print("Cannot close last window!\n");
                             }
                        } else if action == 2 {
                             if win.maximized {
                                 if let Some((x, y, w, h)) = win.saved_rect {
                                     win.x = x; win.y = y; win.width = w; win.height = h;
                                     win.maximized = false; win.saved_rect = None;
                                     win.realloc_buffer(); win.draw_decorations();
                                 }
                             } else {
                                 win.saved_rect = Some((win.x, win.y, win.width, win.height));
                                 win.x = 0; win.y = 0; win.width = width; win.height = height - 30;
                                 win.maximized = true;
                                 win.realloc_buffer(); win.draw_decorations();
                             }
                        } else if win.is_title_bar(mx, my) {
                            is_dragging_local = true;
                            drag_offset_x_local = mx - win.x;
                            drag_offset_y_local = my - win.y;
                        } else {
                            win.handle_mouse(mx, my, btn);
                        }
                    }
                } else if !btn {
                    is_dragging_local = false;
                    let idx = shell_mutex.active_idx;
                    // Check bounds just in case
                    if idx < shell_mutex.windows.len() {
                         shell_mutex.windows[idx].handle_mouse(mx, my, btn);
                    }
                } else if btn && is_dragging_local {
                    let idx = shell_mutex.active_idx;
                    if idx < shell_mutex.windows.len() {
                        let win = &mut shell_mutex.windows[idx];
                        if mx > drag_offset_x_local { win.x = mx - drag_offset_x_local; }
                        if my > drag_offset_y_local { win.y = my - drag_offset_y_local; }
                    }
                }
                
                // Write back drag state
                is_dragging = is_dragging_local;
                drag_offset_x = drag_offset_x_local;
                drag_offset_y = drag_offset_y_local;

                // C. UPDATE TASK MANAGER windows
                for win in shell_mutex.windows.iter_mut() {
                    if win.title == "System Monitor" {
                        shell::Shell::update_monitor(win);
                    } else if win.title == "File Explorer" {
                        shell::Shell::update_explorer(win, &shell_mutex.current_dir);
                    } else if win.title.starts_with("Nano - ") {
                        shell::Shell::update_nano(win, &shell_mutex.nano_status);
                    }
                }

                // --- BUDGET BORDERS (Interrupt-Safe) ---
                let shell_load = x86_64::instructions::interrupts::without_interrupts(|| {
                    let mut sched = scheduler::SCHEDULER.lock();
                    let shell_task = sched.tasks.iter().find(|t| t.name == "Shell");
                    if let Some(t) = shell_task {
                        (t.last_cost * 100).checked_div(t.budget).unwrap_or(0)
                    } else { 0 }
                });

                if let Some(win) = shell_mutex.windows.get_mut(shell_mutex.active_idx) {
                    win.set_load_color(shell_load as usize);
                }

                // D. RENDER EVERYTHING
                let mut draw_list: alloc::vec::Vec<&compositor::Window> = alloc::vec::Vec::new();
                draw_list.push(&taskbar);
                for win in &shell_mutex.windows {
                    draw_list.push(win);
                }
                desktop.render(&draw_list, Some(shell_mutex.active_idx), mx, my);
            } else {
                // Shell is None (Initializing)
                let draw_list: alloc::vec::Vec<&compositor::Window> = alloc::vec![&taskbar];
                desktop.render(&draw_list, None, mx, my);
            }
        } else {
            // Shell is busy - Do NOTHING to preserve the last frame.
            // Rendering only the taskbar here causes all other windows to "vanish" for one frame,
            // creating a flickering effect.
        }


        let end_work = unsafe { core::arch::x86_64::_rdtsc() };
        let elapsed = end_work - start;

        // --- FUEL GAUGE ---
        let mut bar_width = ((elapsed as u128 * width as u128) / FRAME_BUDGET_CYCLES as u128) as usize;
        if bar_width > width { bar_width = width; }
        
        let color = if bar_width < (width * 8 / 10) { 0x0000FF00 } else if bar_width < width { 0x00FFFF00 } else { 0x00FF0000 };
        for y in (height-8)..height {
            for x in 0..width {
                unsafe {
                    let offset = y * pitch + x;
                    if x < bar_width { 
                        *video_ptr.add(offset) = color; 
                    } else {
                        *video_ptr.add(offset) = 0x00222222; // Dark background
                    }
                }
            }
        }

        // --- WAIT FOR FRAME BOUNDARY ---
        while unsafe { core::arch::x86_64::_rdtsc() } - start < FRAME_BUDGET_CYCLES {
            core::hint::spin_loop();
        }
    }
}