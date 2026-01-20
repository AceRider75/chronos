use x86_64::VirtAddr;
use x86_64::structures::tss::TaskStateSegment;
use x86_64::structures::gdt::{GlobalDescriptorTable, Descriptor, SegmentSelector};
use lazy_static::lazy_static;

pub const DOUBLE_FAULT_IST_INDEX: u16 = 0;

lazy_static! {
    static ref TSS: TaskStateSegment = {
        let mut tss = TaskStateSegment::new();
        
        // 1. DEFINE THE KERNEL STACK (RSP0)
        // This is where the CPU jumps when a User Mode app causes an interrupt/crash.
        const STACK_SIZE: usize = 4096 * 5;
        static mut KERNEL_STACK: [u8; STACK_SIZE] = [0; STACK_SIZE];
        let stack_start = VirtAddr::from_ptr(unsafe { &KERNEL_STACK });
        let stack_end = stack_start + STACK_SIZE;
        
        // CRITICAL FIX: Set RSP0
        tss.privilege_stack_table[0] = stack_end;

        // 2. DEFINE THE DOUBLE FAULT STACK (Existing code)
        tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] = {
             static mut DF_STACK: [u8; STACK_SIZE] = [0; STACK_SIZE];
             let df_start = VirtAddr::from_ptr(unsafe { &DF_STACK });
             df_start + STACK_SIZE
        };
        
        tss
    };
}
lazy_static! {
    static ref GDT: (GlobalDescriptorTable, Selectors) = {
        let mut gdt = GlobalDescriptorTable::new();
        let code_selector = gdt.add_entry(Descriptor::kernel_code_segment());
        let data_selector = gdt.add_entry(Descriptor::kernel_data_segment());
        let user_data_selector = gdt.add_entry(Descriptor::user_data_segment());
        let user_code_selector = gdt.add_entry(Descriptor::user_code_segment());
        let tss_selector = gdt.add_entry(Descriptor::tss_segment(&TSS));
        (gdt, Selectors { 
            code_selector, data_selector, user_code_selector, user_data_selector, tss_selector 
        })
    };
}

struct Selectors {
    code_selector: SegmentSelector,
    data_selector: SegmentSelector,
    user_code_selector: SegmentSelector,
    user_data_selector: SegmentSelector,
    tss_selector: SegmentSelector,
}

pub fn init() {
    use x86_64::instructions::tables::load_tss;
    use x86_64::instructions::segmentation::{CS, Segment, DS, SS};

    GDT.0.load();
    unsafe {
        CS::set_reg(GDT.1.code_selector);
        SS::set_reg(GDT.1.data_selector); // Set Stack Segment for Kernel
        load_tss(GDT.1.tss_selector);
    }
}

pub fn get_user_selectors() -> (u16, u16) {
    // RPL 3 is required for Ring 3
    (GDT.1.user_code_selector.0 | 3, GDT.1.user_data_selector.0 | 3)
}