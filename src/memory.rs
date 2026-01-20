use x86_64::structures::paging::{
    OffsetPageTable, PageTable, PageTableFlags, PhysFrame, Size4KiB, Mapper, FrameAllocator
};
use x86_64::{PhysAddr, VirtAddr};
use limine::request::HhdmRequest;

// We need an allocator to create new page tables if needed
// For this tutorial, we cheat and assume the tables exist.
// In a full OS, you need a PMM (Physical Memory Manager) here.

pub unsafe fn init(hhdm_offset: u64) -> OffsetPageTable<'static> {
    let level_4_table_addr = x86_64::registers::control::Cr3::read().0.start_address().as_u64();
    let level_4_table_virt = VirtAddr::new(level_4_table_addr + hhdm_offset);
    let level_4_table = &mut *level_4_table_virt.as_mut_ptr();
    
    OffsetPageTable::new(level_4_table, VirtAddr::new(hhdm_offset))
}

// THE UNLOCKER
// This function tells the CPU: "Allow Ring 3 to read/execute this address"
pub fn mark_as_user(mapper: &mut OffsetPageTable, address: u64) {
    use x86_64::structures::paging::Page;
    
    let page: Page<Size4KiB> = Page::containing_address(VirtAddr::new(address));
    
    // We update the flags for this page
    // SAFETY: This is incredibly dangerous in a real OS. We are modifying active page tables.
    let result = unsafe {
        mapper.update_flags(page, PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE)
    };

    match result {
        Ok(flusher) => flusher.flush(), // Tell CPU to refresh cache
        Err(e) => crate::writer::print("[VMM Error] Failed to update page flags\n"),
    }
}