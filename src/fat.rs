use crate::ata;
use crate::writer;
use alloc::vec::Vec;
use alloc::string::String;

// --- STRUCTS (Packed for Disk Layout) ---

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
struct BPB {
    jmp_boot: [u8; 3],
    oem_name: [u8; 8],
    bytes_per_sector: u16,
    sectors_per_cluster: u8,
    reserved_sectors: u16,
    num_fats: u8,
    root_entry_count: u16,
    total_sectors_16: u16,
    media: u8,
    fat_size_16: u16,
    sectors_per_track: u16,
    num_heads: u16,
    hidden_sectors: u32,
    total_sectors_32: u32,
    // FAT32 Specific
    fat_size_32: u32,
    ext_flags: u16,
    fs_version: u16,
    root_cluster: u32,
    fs_info: u16,
    backup_boot_sector: u16,
    reserved: [u8; 12],
    drive_number: u8,
    reserved1: u8,
    boot_signature: u8,
    volume_id: u32,
    volume_label: [u8; 11],
    fs_type: [u8; 8],
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
struct DirectoryEntry {
    name: [u8; 11],     
    attr: u8,           
    nt_res: u8,
    create_time_tenth: u8,
    create_time: u16,
    create_date: u16,
    access_date: u16,
    cluster_high: u16,
    write_time: u16,
    write_date: u16,
    cluster_low: u16,
    size: u32,
}

pub struct Fat32 {
    drive: ata::AtaDrive,
    partition_offset: u32,
    data_start: u32,       
    sectors_per_cluster: u32,
    root_cluster: u32,
    bytes_per_cluster: u32,
}

impl Fat32 {
    pub fn new() -> Option<Self> {
        let drive = ata::AtaDrive::new(true); // Master
        if !drive.identify() { return None; }

        let sector0 = drive.read_sectors(0, 1);
        let bpb = unsafe { &*(sector0.as_ptr() as *const BPB) };

        // FIX: Accessing fields of packed structs is tricky. 
        // We copy the values out to local variables first.
        // This avoids the "unaligned reference" error.
        let bytes_per_sector = bpb.bytes_per_sector;
        let root_cluster = bpb.root_cluster;
        let reserved_sectors = bpb.reserved_sectors as u32;
        let fat_size = bpb.fat_size_32;
        let num_fats = bpb.num_fats as u32;
        let spc = bpb.sectors_per_cluster as u32;

        if bytes_per_sector != 512 {
            writer::print("[FAT] Error: Non-512 byte sectors not supported.\n");
            return None;
        }

        // CALCULATE OFFSETS
        let fat_area_size = num_fats * fat_size;
        let data_start = reserved_sectors + fat_area_size;
        
        // Use the local variable 'root_cluster', not 'bpb.root_cluster'
        writer::print(&alloc::format!("[FAT] Found Volume. Root Cluster: {}\n", root_cluster));

        Some(Fat32 {
            drive,
            partition_offset: 0,
            data_start,
            sectors_per_cluster: spc,
            root_cluster,
            bytes_per_cluster: spc * 512,
        })
    }

    pub fn list_root(&self) {
        let root_lba = self.cluster_to_lba(self.root_cluster);
        
        // Read one cluster
        let data = self.drive.read_sectors(root_lba, self.sectors_per_cluster as u8);
        
        writer::print("--- HARD DRIVE FILES ---\n");

        for i in (0..data.len()).step_by(32) {
            if i + 32 > data.len() { break; }
            
            let entry = unsafe { &*(data.as_ptr().add(i) as *const DirectoryEntry) };

            if entry.name[0] == 0x00 { break; } // End
            if entry.name[0] == 0xE5 { continue; } // Deleted
            if entry.attr == 0x0F { continue; } // LFN

            // Copy out size (u32) to avoid unaligned reference error
            let size = entry.size;
            
            // Handle Name (It's a byte array, so references are usually safe, but let's be careful)
            let name = core::str::from_utf8(&entry.name).unwrap_or("INVALID");
            let is_dir = (entry.attr & 0x10) != 0;
            
            if is_dir {
                writer::print(&alloc::format!("[DIR]  {}\n", name));
            } else {
                writer::print(&alloc::format!("[FILE] {} ({} bytes)\n", name, size));
            }
        }
    }

    fn cluster_to_lba(&self, cluster: u32) -> u32 {
        self.partition_offset + self.data_start + ((cluster - 2) * self.sectors_per_cluster)
    }
}