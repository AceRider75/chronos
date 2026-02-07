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

/// Gets a fresh physical frame from the system memory map
pub fn alloc_frame() -> PhysAddr {
    unsafe {
        let allocator = FRAME_ALLOCATOR.as_mut().expect("PMM not init");
        let frame = allocator.allocate_frame().expect("OUT OF RAM");
        frame.start_address()
    }
}

/// Maps a page and manually unlocks the entire 4-level hierarchy for Ring 3
pub unsafe fn map_user_page(virt: u64, phys: u64) {
    let hhdm = HHDM;
    let addr = VirtAddr::new(virt);
    let l4_table_phys = x86_64::registers::control::Cr3::read().0.start_address().as_u64();
    let pml4 = &mut *((l4_table_phys + hhdm) as *mut PageTable);

    // Level 4
    let p4_idx = addr.p4_index();
    if pml4[p4_idx].is_unused() {
        let frame = alloc_frame();
        zero_frame(frame.as_u64());
        pml4[p4_idx].set_addr(frame, PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE);
    } else {
        let flags = pml4[p4_idx].flags() | PageTableFlags::USER_ACCESSIBLE | PageTableFlags::WRITABLE;
        pml4[p4_idx].set_flags(flags);
    }

    // Level 3
    let pdpt_phys = pml4[p4_idx].addr();
    let pdpt = &mut *((pdpt_phys.as_u64() + hhdm) as *mut PageTable);
    let p3_idx = addr.p3_index();
    if pdpt[p3_idx].is_unused() {
        let frame = alloc_frame();
        zero_frame(frame.as_u64());
        pdpt[p3_idx].set_addr(frame, PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE);
    } else {
        let flags = pdpt[p3_idx].flags() | PageTableFlags::USER_ACCESSIBLE | PageTableFlags::WRITABLE;
        pdpt[p3_idx].set_flags(flags);
    }

    // Level 2
    let pd_phys = pdpt[p3_idx].addr();
    let pd = &mut *((pd_phys.as_u64() + hhdm) as *mut PageTable);
    let p2_idx = addr.p2_index();
    if pd[p2_idx].is_unused() {
        let frame = alloc_frame();
        zero_frame(frame.as_u64());
        pd[p2_idx].set_addr(frame, PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE);
    } else {
        let flags = pd[p2_idx].flags() | PageTableFlags::USER_ACCESSIBLE | PageTableFlags::WRITABLE;
        pd[p2_idx].set_flags(flags);
    }

    // Level 1
    let pt_phys = pd[p2_idx].addr();
    let pt = &mut *((pt_phys.as_u64() + hhdm) as *mut PageTable);
    pt[addr.p1_index()].set_addr(PhysAddr::new(phys), PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE);

    x86_64::instructions::tlb::flush(addr);
}

/// Maps a kernel page (No Ring 3 access)
pub unsafe fn map_kernel_page(virt: u64, phys: u64) {
    let hhdm = HHDM;
    let addr = VirtAddr::new(virt);
    let l4_table_phys = x86_64::registers::control::Cr3::read().0.start_address().as_u64();
    let pml4 = &mut *((l4_table_phys + hhdm) as *mut PageTable);

    // Level 4
    let p4_idx = addr.p4_index();
    if pml4[p4_idx].is_unused() {
        let frame = alloc_frame();
        zero_frame(frame.as_u64());
        pml4[p4_idx].set_addr(frame, PageTableFlags::PRESENT | PageTableFlags::WRITABLE);
    }

    // Level 3
    let pdpt_phys = pml4[p4_idx].addr();
    let pdpt = &mut *((pdpt_phys.as_u64() + hhdm) as *mut PageTable);
    let p3_idx = addr.p3_index();
    if pdpt[p3_idx].is_unused() {
        let frame = alloc_frame();
        zero_frame(frame.as_u64());
        pdpt[p3_idx].set_addr(frame, PageTableFlags::PRESENT | PageTableFlags::WRITABLE);
    }

    // Level 2
    let pd_phys = pdpt[p3_idx].addr();
    let pd = &mut *((pd_phys.as_u64() + hhdm) as *mut PageTable);
    let p2_idx = addr.p2_index();
    if pd[p2_idx].is_unused() {
        let frame = alloc_frame();
        zero_frame(frame.as_u64());
        pd[p2_idx].set_addr(frame, PageTableFlags::PRESENT | PageTableFlags::WRITABLE);
    }

    // Level 1
    let pt_phys = pd[p2_idx].addr();
    let pt = &mut *((pt_phys.as_u64() + hhdm) as *mut PageTable);
    pt[addr.p1_index()].set_addr(PhysAddr::new(phys), PageTableFlags::PRESENT | PageTableFlags::WRITABLE);

    x86_64::instructions::tlb::flush(addr);
}

unsafe fn zero_frame(phys: u64) {
    let ptr = (phys + HHDM) as *mut u64;
    for i in 0..(4096/8) { core::ptr::write_volatile(ptr.add(i), 0); }
}

pub struct BootFrameAllocator {
    memmap: &'static MemoryMapResponse,
    next_free_frame: usize,
}


impl BootFrameAllocator {
    pub fn new(memmap: &'static MemoryMapResponse) -> Self {
        BootFrameAllocator { memmap, next_free_frame: 0 }
    }

    fn usable_frames(&self) -> impl Iterator<Item = PhysFrame> {
        self.memmap.entries().iter()
            .filter(|e| e.entry_type == EntryType::USABLE)
            // CHANGE: Lower filter to 1MB (0x100_000)
            // Limine protects the kernel/modules automatically, so we don't need to manually skip 16MB.
            .filter(|e| e.base >= 0x100_000) 
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