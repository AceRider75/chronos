use x86_64::structures::paging::{PageTable, PageTableFlags, PhysFrame, Size4KiB, FrameAllocator};
use x86_64::{PhysAddr, VirtAddr};
use limine::response::MemoryMapResponse;
use limine::memory_map::EntryType; 

static mut FRAME_ALLOCATOR: Option<BootFrameAllocator> = None;
static mut HHDM: u64 = 0;

pub unsafe fn init(hhdm_offset: u64, memmap: &'static MemoryMapResponse) {
    HHDM = hhdm_offset;
    FRAME_ALLOCATOR = Some(BootFrameAllocator::new(memmap));
}

/// THE DIVINE OVERRIDE: 
/// Manually builds the path to a page, forcing USER and EXECUTE permissions.
pub unsafe fn map_user_page(virt: u64, phys: u64) {
    let hhdm = HHDM;
    let addr = VirtAddr::new(virt);
    
    let l4_table_phys = x86_64::registers::control::Cr3::read().0.start_address().as_u64();
    let pml4 = &mut *((l4_table_phys + hhdm) as *mut PageTable);

    // 1. Level 4 Entry
    let p4_idx = addr.p4_index();
    if pml4[p4_idx].is_unused() {
        let frame = allocate_frame_raw();
        pml4[p4_idx].set_addr(frame, PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE);
    } else {
        // FIX: Separate the read and write to satisfy borrow checker
        let flags = pml4[p4_idx].flags() | PageTableFlags::USER_ACCESSIBLE;
        pml4[p4_idx].set_flags(flags);
    }

    // 2. Level 3 Entry (PDPT)
    let pdpt = &mut *((pml4[p4_idx].addr().as_u64() + hhdm) as *mut PageTable);
    let p3_idx = addr.p3_index();
    if pdpt[p3_idx].is_unused() {
        let frame = allocate_frame_raw();
        pdpt[p3_idx].set_addr(frame, PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE);
    } else {
        let flags = pdpt[p3_idx].flags() | PageTableFlags::USER_ACCESSIBLE;
        pdpt[p3_idx].set_flags(flags);
    }

    // 3. Level 2 Entry (PD)
    let pd = &mut *((pdpt[p3_idx].addr().as_u64() + hhdm) as *mut PageTable);
    let p2_idx = addr.p2_index();
    if pd[p2_idx].is_unused() {
        let frame = allocate_frame_raw();
        pd[p2_idx].set_addr(frame, PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE);
    } else {
        let flags = pd[p2_idx].flags() | PageTableFlags::USER_ACCESSIBLE;
        pd[p2_idx].set_flags(flags);
    }

    // 4. Level 1 Entry (The Actual Page)
    let pt = &mut *((pd[p2_idx].addr().as_u64() + hhdm) as *mut PageTable);
    let p1_idx = addr.p1_index();
    
    // We explicitly set USER and do NOT set NX (so it remains executable)
    pt[p1_idx].set_addr(PhysAddr::new(phys), PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE);

    // Flush the CPU cache for this address
    x86_64::instructions::tlb::flush(addr);
}

unsafe fn allocate_frame_raw() -> PhysAddr {
    let allocator = FRAME_ALLOCATOR.as_mut().expect("Allocator not init");
    let frame = allocator.allocate_frame().expect("Out of memory");
    // Clear the new table memory to all zeros
    let ptr = (frame.start_address().as_u64() + HHDM) as *mut u8;
    for i in 0..4096 { *ptr.add(i) = 0; }
    frame.start_address()
}

pub struct BootFrameAllocator {
    memmap: &'static MemoryMapResponse,
    next_free_frame: usize,
}

impl BootFrameAllocator {
    pub fn new(memmap: &'static MemoryMapResponse) -> Self {
        BootFrameAllocator { memmap, next_free_frame: 1024 } // Start higher to be safe
    }
    fn usable_frames(&self) -> impl Iterator<Item = PhysFrame> {
        self.memmap.entries().iter()
            .filter(|e| e.entry_type == EntryType::USABLE)
            .flat_map(|e| (0..e.length).step_by(4096).map(move |offset| e.base + offset))
            .map(|addr| PhysFrame::containing_address(PhysAddr::new(addr)))
    }
}

unsafe impl FrameAllocator<Size4KiB> for BootFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame<Size4KiB>> {
        let frame = self.usable_frames().nth(self.next_free_frame);
        self.next_free_frame += 1;
        frame
    }
}