use crate::{writer, state, memory};
use core::sync::atomic::Ordering;

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiHeader {
    pub signature: [u8; 4],
    pub length: u32,
    pub revision: u8,
    pub checksum: u8,
    pub oem_id: [u8; 6],
    pub oem_table_id: [u8; 8],
    pub oem_revision: u32,
    pub creator_id: u32,
    pub creator_revision: u32,
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct Rsdp {
    pub signature: [u8; 8],
    pub checksum: u8,
    pub oem_id: [u8; 6],
    pub revision: u8,
    pub rsdt_addr: u32,
    pub length: u32,
    pub xsdt_addr: u64,
    pub extended_checksum: u8,
    pub reserved: [u8; 3],
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct Fadt {
    pub header: AcpiHeader,
    pub firmware_ctrl: u32,
    pub dsdt: u32,
    pub reserved: u8,
    pub preferred_pm_profile: u8,
    pub sci_interrupt: u16,
    pub smi_command_port: u32,
    pub acpi_enable: u8,
    pub acpi_disable: u8,
    pub s4bios_req: u8,
    pub pstate_control: u8,
    pub pm1a_event_block: u32,
    pub pm1b_event_block: u32,
    pub pm1a_control_block: u32,
    pub pm1b_control_block: u32,
    pub pm2_control_block: u32,
    pub pm_timer_block: u32,
    pub gpe0_block: u32,
    pub gpe1_block: u32,
    pub pm1_event_length: u8,
    pub pm1_control_length: u8,
    pub pm2_control_length: u8,
    pub pm_timer_length: u8,
    pub gpe0_length: u8,
    pub gpe1_length: u8,
    pub gpe1_base: u8,
    pub cstate_control: u8,
    pub worst_c2_latency: u16,
    pub worst_c3_latency: u16,
    pub flush_size: u16,
    pub flush_stride: u16,
    pub duty_offset: u8,
    pub duty_width: u8,
    pub day_alarm: u8,
    pub month_alarm: u8,
    pub century: u8,
    pub boot_architecture_flags: u16,
    pub reserved2: u8,
    pub flags: u32,
}

pub static mut FADT: Option<Fadt> = None;

pub fn init(rsdp_ptr: u64) {
    let hhdm = state::HHDM_OFFSET.load(Ordering::Relaxed);
    
    // 1. Map and parse RSDP
    map_region(rsdp_ptr, 1024); // Map at least a page
    let rsdp = unsafe { &*((rsdp_ptr + hhdm) as *const Rsdp) };

    if &rsdp.signature != b"RSD PTR " {
        writer::print("[ACPI] Error: Invalid RSDP Signature\n");
        return;
    }

    writer::print(&alloc::format!("[ACPI] Revision: {}\n", rsdp.revision));

    let xsdt_addr = if rsdp.revision >= 2 {
        rsdp.xsdt_addr
    } else {
        rsdp.rsdt_addr as u64
    };

    // 2. Map and parse XSDT/RSDT
    map_region(xsdt_addr, 4096);
    let xsdt = unsafe { &*((xsdt_addr + hhdm) as *const AcpiHeader) };
    if &xsdt.signature != b"XSDT" && &xsdt.signature != b"RSDT" {
        writer::print("[ACPI] Error: Invalid XSDT/RSDT Signature\n");
        return;
    }

    let entries = (xsdt.length as usize - core::mem::size_of::<AcpiHeader>()) / if rsdp.revision >= 2 { 8 } else { 4 };
    writer::print(&alloc::format!("[ACPI] Found {} tables\n", entries));

    for i in 0..entries {
        let table_ptr_addr = xsdt_addr + hhdm + core::mem::size_of::<AcpiHeader>() as u64 + (i * if rsdp.revision >= 2 { 8 } else { 4 }) as u64;
        let table_phys = if rsdp.revision >= 2 {
            unsafe { *(table_ptr_addr as *const u64) }
        } else {
            unsafe { *(table_ptr_addr as *const u32) as u64 }
        };

        // 3. Map and parse Table Header
        map_region(table_phys, 4096);
        let header = unsafe { &*((table_phys + hhdm) as *const AcpiHeader) };
        let sig = core::str::from_utf8(&header.signature).unwrap_or("????");
        writer::print(&alloc::format!("[ACPI] Table: {}\n", sig));

        if sig == "FACP" {
            map_region(table_phys, header.length as u64);
            let fadt = unsafe { *((table_phys + hhdm) as *const Fadt) };
            unsafe { FADT = Some(fadt) };
        }
    }
}

/// Helper to map a physical region in the HHDM, ensuring page alignment
fn map_region(phys: u64, size: u64) {
    let hhdm = state::HHDM_OFFSET.load(Ordering::Relaxed);
    let start_page = phys & !0xFFF;
    let end_page = (phys + size + 0xFFF) & !0xFFF;
    
    for page in (start_page..end_page).step_by(4096) {
        unsafe { memory::map_kernel_page(page + hhdm, page); }
    }
}

pub fn shutdown() {
    writer::print("[ACPI] Shutdown initiated...\n");
    // For a real shutdown, we need to:
    // 1. Find the \_S5 object in the DSDT/SSDTs (requires an AML parser)
    // 2. Write the SLP_TYPa and SLP_TYPb values to PM1a_CNT and PM1b_CNT
    
    // AML parsing is very complex. For now, we'll try the QEMU debug port
    // or wait for a future phase with full AML support.
    
    unsafe {
        use x86_64::instructions::port::Port;
        // QEMU/Bochs specific shutdown
        Port::<u16>::new(0x604).write(0x2000);
        // VirtualBox specific shutdown
        Port::<u16>::new(0x4004).write(0x3400);
    }
}
