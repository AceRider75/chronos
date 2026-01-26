

---

# Chronos OS: Development Technical Log
**System Architect** | **January 19, 2026 – January 25, 2026**

---

## Phase 1: A Time-Aware, Visually-Semantic Operating System
**Date:** January 19, 2026

### Abstract
This report details the initial development phase of **Chronos**, an experimental operating system built from scratch in Rust. Unlike general-purpose operating systems which prioritize throughput and fairness, Chronos prioritizes *strict timing contracts*. The core philosophy is that missing a deadline is a system failure, not a performance artifact. This document covers the boot process, the implementation of visual semantics for CPU cycle budgeting, and the establishment of the Interrupt Descriptor Table (IDT).

### 1. Philosophy and Architecture
#### 1.1 The "Time is Primary" Concept
Modern operating systems (Linux, Windows) utilize "Best Effort" scheduling. If a task requires more CPU time than available, the User Interface (UI) typically lags or freezes. Chronos inverts this paradigm:
*   **The Frame is God:** The system is synchronized to the refresh rate of the display.
*   **Contractual Execution:** Applications must declare a time budget.
*   **Visual Semantics:** System load is not a number in a task manager; it is a visual element of the desktop environment itself.

#### 1.2 Toolchain
The kernel is developed in a "Bare Metal" environment without the standard library (`no_std`).
*   **Language:** Rust (Nightly channel for `abi_x86_interrupt`).
*   **Bootloader:** Limine (v0.5) utilizing the Stivale2 protocol for framebuffer acquisition.
*   **Target:** `x86_64-unknown-none`.
*   **Emulator:** QEMU (kvm accelerated).

### 2. Implementation: The Visual Kernel
#### 2.1 Bootstrapping (Limine)
We bypass the legacy VGA text mode (0xB8000) and jump directly to a Linear Framebuffer. The kernel requests a graphical screen from the bootloader immediately upon entry.

```rust
#[used]
static FRAMEBUFFER_REQUEST: FramebufferRequest = FramebufferRequest::new();

// In _start():
let video_ptr = framebuffer.addr() as *mut u32;
// Direct pixel manipulation is now possible
```

#### 2.2 The Main Loop and Time Measurement
Chronos does not sleep. The main kernel loop is a continuous process that draws frames. We utilize the CPU's `RDTSC` (Read Time-Stamp Counter) instruction to measure the exact number of cycles consumed by the render pass.

$$ Cost = T_{end} - T_{start} $$

#### 2.3 Visual Semantics: The Fuel Gauge
Instead of logging performance data to a file, Chronos visualizes it in real-time. A "Fuel Gauge" is drawn at the top of the screen.
*   **Green:** Usage is within the defined budget (Safety Margin).
*   **Yellow:** Usage is approaching the limit.
*   **Red:** The deadline was missed (OS Failure State).

```rust
let cycle_budget: u64 = 2_500_000;
let elapsed = end_time - start_time;

let mut bar_width = ((elapsed as u128 * width as u128) / cycle_budget as u128) as usize;

let usage_color = if bar_width < width / 2 {
    0x0000FF00 // Green
} else {
    0x00FF0000 // Red (Failure)
};
```

### 3. Interrupt Handling
#### 3.1 The IDT (Interrupt Descriptor Table)
To move beyond a simple loop, the OS must react to asynchronous hardware events. We implemented the IDT using the `x86_64` crate. We established a handler for the **Breakpoint Exception (Vector 3)**. This allows the kernel to pause execution, handle an event, and resume, preventing a Triple Fault (reboot).

```rust
lazy_static! {
    static ref IDT: InterruptDescriptorTable = {
        let mut idt = InterruptDescriptorTable::new();
        idt.breakpoint.set_handler_fn(breakpoint_handler);
        idt
    };
}
```

#### 3.2 Verification
To verify the nervous system of the OS, we triggered a software interrupt:
```rust
x86_64::instructions::interrupts::int3();
```
**Result:** The OS drew a single white line at `y = 0` (as programmed in our visual debugging logic), confirming that the CPU successfully jumped to the handler and returned to the main loop without crashing.

### 4. Observations and "The Jitter"
During testing in QEMU (hosted on Arch Linux), we observed significant fluctuation in the Visual Fuel Gauge. Even with a static workload, the bar "jitters" into the red zone periodically.
**Analysis:** This visualizes the latency introduced by the host OS scheduler and the emulator overhead. Chronos is effectively acting as a *Latency Visualizer* for the underlying hardware/hypervisor stack.

---

## Phase 2: The Nervous System (Interrupts, Input, and Concurrency)
**Date:** January 19, 2026

### 1. Overview
A functional operating system must respond to external stimuli (Timer, Keyboard, Disk). Phase 2 focused on connecting the "Brain in a Jar" to the hardware via the x86 Interrupt System.

### 2. The Interrupt Architecture
#### 2.1 The IDT (Interrupt Descriptor Table)
We utilized the `x86_64` crate to define handlers for:
*   **Vector 3:** Breakpoint Exception (Debug).
*   **Vector 32:** System Timer (IRQ 0).
*   **Vector 33:** Keyboard (IRQ 1).

#### 2.2 The 8259 PIC Remapping
A critical legacy issue on x86 architecture is that the 8259 PIC maps hardware interrupts to vectors 0-15 by default. These conflict with CPU internal exceptions (e.g., Double Fault is Vector 8).
**Solution:** We remapped the Master PIC to offset 32 and the Slave PIC to offset 40.

```rust
pub const PIC_1_OFFSET: u8 = 32;
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

pub static PICS: Mutex<ChainedPics> = Mutex::new(unsafe {
    ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET)
});
```

### 3. Input Handling and Diagnostics
#### 3.1 The "Deaf CPU" Problem
**Diagnosis:** The PIC lines were masked (muted) by default.
**Resolution:** We implemented an explicit unmasking routine, writing `0xFC` to the Master PIC data port, which enables lines 0 (Timer) and 1 (Keyboard).

#### 3.2 PS/2 Keyboard Driver
We implemented a driver that listens on I/O Port `0x60`. Upon receiving an interrupt at Vector 33, the handler:
1.  Locks the Keyboard Mutex.
2.  Reads the scancode byte from Port `0x60`.
3.  Decodes the scancode into a Rust `char`.
4.  Sends an "End of Interrupt" (EOI) signal to the PIC.

### 4. State Management: Interactive Time
#### 4.2 The Atomic Solution
To modify the system's "Time Budget" in real-time without data races, we utilized `AtomicU64`.

```rust
// Shared Global State
pub static CYCLE_BUDGET: AtomicU64 = AtomicU64::new(2_500_000);

// In the Interrupt Handler
match character {
    '+' => state::adjust_budget(1_000_000), // Relax Budget
    '-' => state::adjust_budget(-1_000_000), // Tighten Budget
}
```

---

## Phase 3: Visual Output (Framebuffers & Text)
**Date:** January 19, 2026

### 1. Objective
Phase 3 aimed to implement a `print!` style capability without access to the standard library or an underlying OS.

### 2. The Graphics Stack
*   **Format:** 32-bit TrueColor (`0x00RRGGBB`).
*   **Access:** Direct memory mapping via raw pointers (`*mut u32`).
*   **Font:** Incorporated `noto-sans-mono-bitmap` crate. Our engine "blits" pixel brightness values into the framebuffer.

### 3. The Writer Implementation
#### 3.2 Rust Safety Challenges
The Framebuffer is accessed via a raw pointer (`*mut u32`). Rust's safety model treats raw pointers as not thread-safe (`!Send` and `!Sync`). Attempting to put a struct containing a raw pointer into a static Mutex results in a compiler error.
**Solution:** We manually implemented the safety traits.

```rust
pub struct Writer {
    pub video_ptr: *mut u32,
    // ...
}

// SAFETY: We guarantee that Writer is only accessed
// through a Mutex, preventing data races.
unsafe impl Send for Writer {}
unsafe impl Sync for Writer {}
```

### 4. System Integration
#### 4.1 The Visual Layout
*   **Top Zone (0-150px):** Dedicated Console Output.
*   **Middle Zone (150-200px):** The "Fuel Gauge" (Time Budget visualizer).
*   **Bottom Zone (200px+):** The "Void" (Background heartbeat animation).

---

## Phase 4: Dynamic Memory Management (The Heap)
**Date:** January 19, 2026

### 1. Objective
The goal of Phase 4 was to initialize a Dynamic Memory Manager to enable the use of standard Rust collections: `Box<T>`, `Vec<T>`, and `String`.

### 2. Allocator Implementation
#### 2.1 Strategy: The "Static Array" Heap
We reserve a fixed 100 KiB array inside the kernel's binary (specifically in the `.bss` section).

```rust
// Reserve 100 KiB of zeroed memory
pub const HEAP_SIZE: usize = 100 * 1024;
static mut HEAP_MEM: [u8; HEAP_SIZE] = [0; HEAP_SIZE];
```

#### 2.2 The Linked List Allocator
We utilized the `linked_list_allocator` crate wrapped in a `LockedHeap`.

```rust
use linked_list_allocator::LockedHeap;

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

pub fn init_heap() {
    unsafe {
        let heap_start = HEAP_MEM.as_ptr() as usize;
        ALLOCATOR.lock().init(heap_start as *mut u8, HEAP_SIZE);
    }
}
```

### 4. Verification
#### 4.1 Test Cases
1.  **Box Allocation:** Placing a single integer on the heap.
2.  **Vector Growth:** Creating a `Vec<usize>` and pushing elements to trigger a dynamic resize.
3.  **String Formatting:** Using the `format!` macro.

```rust
// Test Vector Growth
let mut vec = Vec::new();
for i in 0..5 {
    vec.push(i);
}
// Test String Formatting
let msg = format!("[INFO] Vector Size: {}\n", vec.len());
writer::print(&msg);
```

---

## Phase 5: The Time-Aware Cooperative Scheduler
**Date:** January 19, 2026

### 1. Overview
The Scheduler’s job is not just to run code, but to audit the performance of that code against its contract.

### 2. Implementation
#### 2.1 The Task Structure
```rust
pub type Job = fn();

pub struct Task {
    pub name: String,
    pub budget: u64,
    pub job: Job,
    pub last_cost: u64, // Measurement from previous frame
    pub status: TaskStatus,
}

pub enum TaskStatus {
    Waiting,
    Success, // Cost <= Budget
    Failure, // Cost > Budget
}
```

#### 2.2 The Scheduler Logic
The Scheduler maintains a `Vec<Task>`. In every frame, it iterates through this vector.
**The Measurement Process:**
1.  Sample TSC: Read CPU timestamp ($T_{start}$).
2.  Execute: Run the function pointer (`task.job()`).
3.  Sample TSC: Read CPU timestamp ($T_{end}$).
4.  Audit: Compare ($T_{end} - T_{start}$) against `task.budget`.

```rust
for task in self.tasks.iter_mut() {
    let start = unsafe { _rdtsc() };
    (task.job)();
    let end = unsafe { _rdtsc() };

    let cost = end - start;
    if cost <= task.budget {
        task.status = TaskStatus::Success;
    } else {
        task.status = TaskStatus::Failure;
    }
}
```

### 3. Verification Testing
We configured a task with a budget of **1 cycle** ("Impossible Mode"). The task correctly reported `[ FAIL ]`, confirming the audit logic works.

---

## Phase 6: The Interactive Shell
**Date:** January 19, 2026

### 2. The Input Architecture
#### 2.1 The Producer-Consumer Problem
Keyboard interrupts fire asynchronously. If we processed commands inside the Handler, we would block the CPU.
**Solution:** We implemented a Ring Buffer (FIFO) using `VecDeque`, wrapped in a `spin::Mutex`.

```rust
lazy_static! {
    pub static ref KEYBOARD_BUFFER: Mutex<VecDeque<char>> = Mutex::new(VecDeque::new());
}

pub fn push_key(c: char) {
    x86_64::instructions::interrupts::without_interrupts(|| {
        KEYBOARD_BUFFER.lock().push_back(c);
    });
}
```

### 3. The Shell Implementation
The Shell is a persistent task in the Scheduler.
*   **Standard Char:** Appended to buffer and printed.
*   **Newline (\n):** Triggers `execute_command()`.
*   **Backspace (\x08):** Triggers visual deletion logic.

```rust
// Budget: 100,000 cycles (High Priority)
chronos_scheduler.add_task("Shell", 100_000, shell::shell_task);
```

### 4. Visual Upgrades
**Persistence Fix:** Removed `writer.clear()` from the main loop to prevent typed text from vanishing.
**Backspace Logic:** Implemented a method that moves the cursor back and draws a background-colored rectangle.

---

## Phase 7: Data Persistence (Ramdisk Filesystem)
**Date:** January 19, 2026

### 2. System Architecture
Directly implementing NVMe/SATA drivers was outside the immediate scope. We utilized a **Ramdisk** approach:
1.  **Packaging:** Files (e.g., `welcome.txt`) are placed in the ISO root.
2.  **Handover:** Bootloader loads files into RAM.
3.  **Discovery:** Kernel parses `ModuleRequest` to find physical addresses.

### 3. Implementation Details
```rust
let start = module.addr() as *const u8;
let size = module.size() as usize;

// Create a static slice representing the file's data
let data = unsafe { core::slice::from_raw_parts(start, size) };
```

### 4. User Interaction
Extended the Shell with `ls` (list files) and `cat <filename>` (print content). Verification confirmed we could read `welcome.txt` from the virtual disk.

---

## Phase 8: Hardware Isolation (Ring 3 Transition)
**Date:** January 20, 2026

### 1. Overview
The objective was to transition from Ring 0 (Kernel) to Ring 3 (User), where privileged instructions like `cli` and `hlt` are prohibited.

### 2. GDT and TSS
*   **GDT:** Defined User Code and User Data segments (Privilege Level 3).
*   **TSS (Task State Segment):** Populated the `RSP0` field. This prevents Triple Faults during interrupts by providing a safe stack for the CPU to switch to when returning to Ring 0.

### 4. Ring Transition Logic (`iretq`)
We utilize inline assembly to fake a return from interrupt.

```rust
unsafe {
    asm!(
        "cli",              // Disable interrupts
        "mov ds, ax",       // Load user data selectors
        "mov es, ax",
        // ...
        "push rax",         // SS (User Data Segment)
        "push rsi",         // RSP (User Stack Pointer)
        "push 0x202",       // RFLAGS (Interrupt Flag set)
        "push rdi",         // CS (User Code Segment)
        "push rdx",         // RIP (Entry point)
        "iretq",            // Perform privilege switch
    );
}
```

### 5. Verification
After transition, invoking a kernel function `user_mode_app` triggered a **Page Fault (Vector 14)**. This confirmed correct privilege enforcement.

---

## Phase 9: Virtual Memory Management
**Date:** January 20, 2026

### 1. The Memory Protection Barrier
Upon entering Ring 3, the CPU enforces Paging permissions. Executing code marked as "Supervisor Only" causes a Page Fault.

### 2. VMM Implementation
#### 2.1 HHDM (Higher Half Direct Map)
To modify Page Tables, we need their Virtual Addresses. We used the Limine `HhdmRequest` to map physical RAM to a known virtual offset.

#### 2.2 Bit Manipulation
We implemented `mark_as_user` to set the specific bits in the Page Table Entries (PTE).

```rust
pub fn mark_as_user(mapper: &mut OffsetPageTable, address: u64) {
    let page = Page::containing_address(VirtAddr::new(address));

    // Set the USER_ACCESSIBLE bit (Bit 2)
    let flags = PageTableFlags::PRESENT
        | PageTableFlags::WRITABLE
        | PageTableFlags::USER_ACCESSIBLE;

    unsafe {
        mapper.update_flags(page, flags).unwrap().flush();
    }
}
```

### 3. Results
After unlocking the memory page containing the user function, the system entered a stable state inside the user loop without crashing.

---

## Phase 10: System Calls
**Date:** January 20, 2026

### 2. Implementation
#### 2.1 IDT Configuration
We modified the IDT entry for Vector 128 (`0x80`) to explicitly allow Ring 3 access.
```rust
use x86_64::PrivilegeLevel;

// Allow Ring 3 (User) to trigger this interrupt
idt[0x80]
    .set_handler_fn(syscall_handler)
    .set_privilege_level(PrivilegeLevel::Ring3);
```

#### 2.2 The User Stack Issue
**The Fix:** We allocated a dedicated `USER_STACK` array and used the VMM to set the `USER_ACCESSIBLE` flag on that page before the jump.

```rust
// Unlock Stack (The missing link)
let stack_addr = unsafe { &USER_STACK as *const _ as u64 };
memory::mark_as_user(&mut mapper, stack_addr);
```

### 3. The Execution Chain
1.  **Transition:** `iretq` to Ring 3.
2.  **Action:** User App executes `asm!("int 0x80")`.
3.  **Trap:** CPU switches to Ring 0 (`syscall_handler`).
4.  **Result:** Kernel prints "User requested Hello World".

---

## Phases 11 & 12: Hardware Enumeration & Network Driver
**Date:** January 20, 2026

### 1. Phase 11: PCI Enumeration
We implemented a brute-force scan of the PCI bus (Ports `0xCF8` / `0xCFC`). The scan identified a device with **Vendor ID 0x10EC** (Realtek) and **Device ID 0x8139**.

### 2. Phase 12: The RTL8139 Driver
**Initialization:**
1.  Enable Bus Mastering (PCI Command Register).
2.  Power On (Command Register `0x37`).
3.  Software Reset.
4.  Unlock Config (Promiscuous Mode).

### 3. The Memory Management Challenge
The Network Card requires Physical Addresses for DMA, but the Kernel uses Virtual Addresses.
**Solution:** We bypassed the Heap allocator for DMA buffers and selected a safe region of Physical RAM at `0x02000000` (32MB mark).

```rust
const RX_BUFFER_PHYS: u32 = 0x0200_0000;
// Calculate Virtual Address for CPU
let rx_virt = hhdm + (RX_BUFFER_PHYS as u64);
// Send Physical Address to NIC
rbstart_port.write(RX_BUFFER_PHYS);
```

### 6. Verification
**TX:** Transmit Status Register returned `OK`.
**RX:** After broadcasting an ARP Request, the Receive Buffer populated with raw data.

---

## Phases 13 & 14: TCP/IP Protocol Stack (ARP, DHCP)
**Date:** January 20, 2026

### 1. Architectural Challenges
*   **Alignment:** Utilized `#[repr(packed)]` on all protocol structs to match wire format.
*   **Endianness:** Implemented `ntohs` to convert Big Endian (Network) to Little Endian (x86).

### 2. Phase 13: ARP
When the driver receives EtherType `0x0806`, it dispatches to the ARP handler.
**Verification:** Successfully intercepted an ARP Reply from the Gateway (`10.0.2.2`).

### 3. Phase 14: DHCP
We constructed a 272-byte DHCP Discover packet (UDP Port 68 -> 67).
**Checksum:** Implemented standard RFC 791 IPv4 Checksum (One's Complement Sum).

```rust
fn calc_ip_checksum(&self, header: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    // 1. Sum words
    for i in (0..header.len()).step_by(2) {
        let word = ((header[i] as u32) << 8) | (header[i+1] as u32);
        sum = sum.wrapping_add(word);
    }
    // 2. Handle Carry & 3. Invert
    while (sum >> 16) != 0 { sum = (sum & 0xFFFF) + (sum >> 16); }
    !sum as u16
}
```

**Result:**
`[NET] DHCP OFFER RECEIVED! >>> ASSIGNED IP: 10.0.2.15 <<<`

---

## Phases 15 & 16: User Mode & Networking
**Date:** January 20, 2026

### 2. Phase 15: Bidirectional IP Networking
**ARP Deadlock:** Inbound traffic was dropped because the Gateway didn't know Chronos's MAC address.
**Fix:** Implemented a **Reactive ARP Responder**. When queried, Chronos now replies with its MAC. This allowed `ping` (ICMP) to work bidirectionally.

### 3. Phase 16: ELF Loading
**Objective:** Execute externally compiled ELF binaries in Ring 3.
**Hierarchical Gating:** We implemented a page table traversal routine to set the `USER` bit on *all* parent entries (PML4, PDPT, PD) leading to the application's address.
**Verification:**
1.  Shell invokes `run testapp`.
2.  Kernel loads ELF to `0x400000`.
3.  Execution transitions to Ring 3.
4.  App triggers `int 0x80`.

---

## Phase 17: PS/2 Mouse Driver
**Date:** January 21, 2026

### 1. Hardware Architecture
Initialized the **8042 Controller**.
**Cascade Interrupt:** Unmasked IRQ 2 on the Master PIC to allow the Slave PIC (IRQ 12, Mouse) to fire.

### 3. Graphics and Rendering
**The "Trail" Problem:** The cursor left permanent white pixels.
**Solution (Save-Draw-Restore):**
1.  **Restore:** Write saved buffer to `prev_x, prev_y`.
2.  **Save:** Read screen pixels at `new_x, new_y`.
3.  **Draw:** Write cursor pixels.

### 4. Concurrency (Deadlock)
A deadlock occurred when the Mouse Interrupt fired while the Main Loop held the `WRITER` lock.
**Fix:** Used `try_lock()`. If the screen is busy, the mouse skips drawing that frame.

```rust
if let Some(mut w) = writer::WRITER.try_lock() {
    // Perform drawing operations...
}
```

---

## Phases 18 & 19: The Compositor & Window Manager
**Date:** January 21, 2026

### 1. Phase 18: The Compositor
**Double Buffering:** Implemented a Backbuffer in RAM. All drawing happens there, then `memcpy` to VRAM to eliminate flicker.
**Heap Crisis:** Allocating the 3MB backbuffer caused OOM.
**Fix:** Increased Kernel Heap in `allocator.rs` from 100 KiB to **32 MiB**.

### 2. Phase 19: The Window Manager
Defined a `Window` struct, each owning its own pixel buffer (`Vec<u32>`).
**Shell Integration:** The Shell now owns a `Window` instance. `print!` calls write to the window's private buffer instead of global VRAM.
**Render Pipeline:**
1.  Clear Backbuffer.
2.  Render Taskbar.
3.  Render Shell Window.
4.  Overlay Cursor.
5.  Flip to VRAM.

---

## Phases 20 & 21: Interactivity & RTC
**Date:** January 21, 2026

### 1. Phase 20: Interactive Windows
**Hit Testing:** Implemented bounding-box checks (`contains(px, py)`).
**Dragging:** Implemented offset-based drag logic to prevent the window from "snapping" to the top-left corner of the mouse.
$$ Window_{new\_x} = Mouse_{current\_x} - \Delta x $$

### 2. Phase 21: Real-Time Clock (RTC)
Read from CMOS ports `0x70/0x71`.
**BCD Decoding:** Converted Binary Coded Decimal to integers.
$$ Binary = (Value \& 0x0F) + ((Value / 16) * 10) $$
Integrated a live digital clock into the Taskbar.

---

## Phase 22: Multi-Tasking GUI
**Date:** January 21, 2026

### 1. Window Decorations
Added "Chrome": Title Bar (Navy Blue, 20px) and Borders.

### 2. Multi-Window Architecture
Refactored `Shell` to manage a vector of windows.
```rust
pub struct Shell {
    pub windows: Vec<compositor::Window>,
    pub active_idx: usize, // The window receiving Keyboard Input
}
```

### 3. Z-Ordering
**Render Pass:** Iterates through the window list from 0 to N. Newer windows are drawn last, appearing "on top."
**Focus Logic:** Reverse-iterator search on mouse click to find the topmost window under the cursor.

---

## Phases 23 & 24: Writable VFS & System Monitoring
**Date:** January 22, 2026

### 1. Phase 23: Writable VFS
Migrated from read-only Limine modules to a **Dynamic Heap Model** (`Vec<File>`) wrapped in a global Mutex.
Implemented `touch`, `rm`, and `write` commands in the Shell.

### 2. Phase 24: The System Monitor
Refactored the Scheduler to be globally accessible (`static Mutex<Scheduler>`).
**"Top" Command:** Spawns a floating window that queries `last_cost` from the Scheduler and renders ASCII bars to visualize CPU usage per task.

**Concurrency Fix:** Decoupled rendering. The Shell task releases the Scheduler lock *before* acquiring the Shell lock to print, preventing recursive deadlocks.

---

## Phases 25-28: Persistent Storage (ATA & FAT32)
**Date:** January 23, 2026

### 1. Phase 25: ATA Hardware Driver
Implemented a PIO (Programmed I/O) driver for Parallel ATA.
**Handshake:** Wait for Busy Clear -> Select Drive/LBA -> Write Command `0x20` -> Read Data.

### 2. Phase 26: FAT32 Architecture
Utilized `#[repr(packed)]` to map raw disk bytes to Rust structs. Resolved alignment exceptions by copying fields to local variables before use.

### 3. Phase 27: File Access
Implemented `lsdisk` to parse the Root Directory. Added 8.3 filename normalization (e.g., "TESTAPP ELF" -> "testapp.elf").

### 4. Phase 28: Execution from Storage
**Problem:** The heap is not guaranteed to be contiguous physical memory, making it hard to map to User Space.
**Solution:**
1.  **Allocate:** Ask PMM for fresh 4KB physical frames.
2.  **Map:** Map frames to `0x400000`.
3.  **Copy:** `memcpy` file data from the Heap buffer to the new virtual address.
4.  **Execute:** `rundisk <filename>`.

---

## Phases 29-32: The Multitasking Sprint
**Date:** January 25, 2026

### 1. Phase 29: Scheduler Overhaul
Transitioned from function pointers to a stack-based **Process** model.
**Stack Forging:** Manually writing a fake stack frame (Registers, Return Address) so the `context_switch` assembly routine can "return" into a new thread for the first time.
**Context Switch (ASM):** Swaps `RSP` and callee-saved registers (`RBX`, `RBP`, `R12-R15`).

### 2. Phase 30: Window Manager (Deadlock Resolution)
**Root Cause:** Circular dependency. Shell owned Window; GUI thread locked Shell to render; Shell thread locked itself to process input.
**Solution:** Decoupled GUI from Logic. Introduced `window_manager.rs`.
```rust
lazy_static! {
    pub static ref WINDOWS: Mutex<Vec<Window>> = Mutex::new(Vec::new());
}
```

### 3. Phase 31: Unified Shell
Refactored Shell to be multi-instance. Input is routed only to `ACTIVE_WINDOW`. The `term` command spawns new terminals dynamically.

### 4. Phase 32: System Integration
**Filesystem:** `ls`, `touch`, `write`, `rm`, `cat`.
**Hard Drive:** `lsdisk`, `catdisk`, `rundisk`.
**Background Execution:** `rundisk` now spawns a user process in the background, allowing the GUI to remain responsive while the binary executes.

```rust
scheduler::SCHEDULER.lock().add_user_process(
    filename, entry_point, stack_top
);
```

### Conclusion
Chronos OS has successfully transitioned to a Multitasking Operating System. It separates the Graphical User Interface from the Command Line Logic, allows for concurrent execution of system tasks, and provides a robust set of tools for managing files and processes.
