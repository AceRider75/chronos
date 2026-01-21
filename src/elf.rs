use crate::writer;
use alloc::format;

#[repr(C, packed)]
struct ElfHeader {
    magic: [u8; 4],
    class: u8,
    data: u8,
    version: u8,
    osabi: u8,
    abiversion: u8,
    pad: [u8; 7],
    e_type: u16,
    machine: u16,
    e_version: u32,
    entry_point: u64,
    phoff: u64,
    shoff: u64,
    flags: u32,
    ehsize: u16,
    phentsize: u16,
    phnum: u16,
    shentsize: u16,
    shnum: u16,
    shstrndx: u16,
}

pub fn run_elf(data: &[u8]) {
    let header = unsafe { &*(data.as_ptr() as *const ElfHeader) };

    if header.magic != [0x7f, 0x45, 0x4c, 0x46] {
        writer::print("[ELF] Error: Invalid Magic Number.\n");
        return;
    }

    // --- THE CRITICAL FIX ---
    // Instead of jumping to the absolute number '1280', 
    // we jump to: (Start of File in RAM) + 1280
    let file_start_addr = data.as_ptr() as u64;
    let entry_point_offset = header.entry_point;
    
    // Most small bare-metal ELFs use the entry point as an absolute address (e.g. 0x400000).
    // But since our test app is PIC (Position Independent), we treat it as an offset if it's small.
    let actual_jump_addr = if entry_point_offset < 0x100000 {
        file_start_addr + entry_point_offset
    } else {
        entry_point_offset
    };

    writer::print(&format!("[ELF] File at: {:x}\n", file_start_addr));
    writer::print(&format!("[ELF] Entry Offset: {:x}\n", entry_point_offset));
    writer::print(&format!("[ELF] Jumping to: {:x}\n", actual_jump_addr));

    unsafe {
        let entry: extern "C" fn() -> ! = core::mem::transmute(actual_jump_addr);
        entry();
    }
}