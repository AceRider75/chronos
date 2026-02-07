use crate::{writer, memory, state};
use alloc::format;
use core::sync::atomic::Ordering;

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

#[repr(C, packed)]
struct ProgramHeader {
    p_type: u32,
    p_flags: u32,
    p_offset: u64,
    p_vaddr: u64,
    p_paddr: u64,
    p_filesz: u64,
    p_memsz: u64,
    p_align: u64,
}

const PT_LOAD: u32 = 1;

pub fn load_and_run(data: &[u8]) {
    let header = unsafe { &*(data.as_ptr() as *const ElfHeader) };

    if header.magic != [0x7f, 0x45, 0x4c, 0x46] {
        crate::serial_print!("[ELF] Error: Invalid Magic Number.\n");
        return;
    }
    if header.class != 2 { // ELF64
        crate::serial_print!("[ELF] Error: Not 64-bit.\n");
        return;
    }
    if header.e_type != 2 && header.e_type != 3 { // EXEC or DYN
        crate::serial_print!("[ELF] Error: Not executable.\n");
        return;
    }

    let hhdm = state::HHDM_OFFSET.load(Ordering::Relaxed);
    let ph_offset = header.phoff as usize;
    let ph_count = header.phnum as usize;
    let ph_size = header.phentsize as usize;

    crate::serial_print!("[ELF] Loading {} segments...\n", ph_count);

    for i in 0..ph_count {
        let offset = ph_offset + (i * ph_size);
        if offset + core::mem::size_of::<ProgramHeader>() > data.len() {
             crate::serial_print!("[ELF] Error: PHDR out of bounds.\n");
             return;
        }
        
        let ph = unsafe { &*(data.as_ptr().add(offset) as *const ProgramHeader) };
        
        if ph.p_type == PT_LOAD {
            // Found a loadable segment
            // writer::print(&format!("[ELF] LOAD: Virt={:x}, FileSz={:x}, MemSz={:x}\n", ph.p_vaddr, ph.p_filesz, ph.p_memsz));

            if ph.p_memsz == 0 { continue; }

            let start_vaddr = ph.p_vaddr;
            let end_vaddr = start_vaddr + ph.p_memsz;
            
            // Align to 4KB pages
            let start_page = start_vaddr & !0xFFF;
            let end_page = (end_vaddr + 0xFFF) & !0xFFF;
            let page_count = (end_page - start_page) / 4096;

            unsafe {
                for p in 0..page_count {
                    let vaddr = start_page + (p * 4096);
                    let frame = memory::alloc_frame();
                    memory::map_user_page(vaddr, frame.as_u64());
                    
                    // Destination pointer (virtual address view for kernel, via HHDM)
                    let dst_ptr = (frame.as_u64() + hhdm) as *mut u8;
                    
                    // Zero the page first (handles BSS implicitly)
                    core::ptr::write_bytes(dst_ptr, 0, 4096);

                    // Calculations for how much file data to copy into *this specific page*
                    let page_end_vaddr = vaddr + 4096;
                    
                    // Does the segment data overlap with this page?
                    // Segment Data range: [ph.p_vaddr, ph.p_vaddr + ph.p_filesz)
                    let seg_data_start = ph.p_vaddr;
                    let seg_data_end = ph.p_vaddr + ph.p_filesz;

                    // Intersection of [vaddr, page_end_vaddr) and [seg_data_start, seg_data_end)
                    let copy_start_v = core::cmp::max(vaddr, seg_data_start);
                    let copy_end_v = core::cmp::min(page_end_vaddr, seg_data_end);

                    if copy_start_v < copy_end_v {
                        let copy_len = (copy_end_v - copy_start_v) as usize;
                        let src_offset = (ph.p_offset + (copy_start_v - ph.p_vaddr)) as usize;
                        let dst_offset = (copy_start_v - vaddr) as usize; // Check alignment within page

                        if src_offset + copy_len <= data.len() {
                             core::ptr::copy_nonoverlapping(
                                data.as_ptr().add(src_offset),
                                dst_ptr.add(dst_offset),
                                copy_len
                            );
                        }
                    }
                }
            }
        }
    }

    let entry_point = header.entry_point;
    crate::serial_print!("[ELF] Entry Point: {:x}\n", entry_point);
    
    // Spawn in a separate task so Shell doesn't die!
    crate::scheduler::SCHEDULER.lock().add_task("UserApp", 1_000_000, 
        crate::shell::Shell::run_user_trampoline, 
        entry_point
    );
}