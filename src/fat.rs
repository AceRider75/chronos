use crate::ata;
use crate::writer;
use alloc::vec::Vec;
use alloc::string::String;
use alloc::format;
use core::convert::TryInto;

// --- STRUCTS ---
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
    fat_start: u32,
}

impl Fat32 {
    pub fn new() -> Option<Self> {
        let drive = ata::AtaDrive::new(true);
        if !drive.identify() { return None; }

        let sector0 = drive.read_sectors(0, 1);
        if sector0.is_empty() {
            writer::print("[FAT] Error: Could not read boot sector.\n");
            return None;
        }
        let bpb = unsafe { &*(sector0.as_ptr() as *const BPB) };

        // Copy packed values to avoid unaligned access
        let bytes_per_sec = bpb.bytes_per_sector;
        let rsvd_sec = bpb.reserved_sectors as u32;
        let num_fats = bpb.num_fats as u32;
        let fat32_size = bpb.fat_size_32;
        let root_cluster = bpb.root_cluster;
        let spc = bpb.sectors_per_cluster as u32;

        if bytes_per_sec != 512 {
            writer::print(&format!("[FAT] Error: Non-512 byte sectors (found {}).\n", bytes_per_sec));
            return None;
        }

        let fat_area_size = num_fats * fat32_size;
        let data_start = rsvd_sec + fat_area_size;
        let fat_start = rsvd_sec;

        writer::print(&format!("[FAT] Mounted. Root Cluster: {}\n", root_cluster));

        Some(Fat32 {
            drive,
            partition_offset: 0,
            data_start,
            sectors_per_cluster: spc,
            root_cluster,
            fat_start,
        })
    }

    // Helper: 8.3 filename ("README  TXT") -> ("README.TXT")
    fn format_name(raw: &[u8; 11]) -> String {
        let name = core::str::from_utf8(&raw[0..8]).unwrap_or("").trim();
        let ext = core::str::from_utf8(&raw[8..11]).unwrap_or("").trim();
        if ext.is_empty() {
            String::from(name)
        } else {
            format!("{}.{}", name, ext)
        }
    }

    pub fn list_root(&self) {
        let root_lba = self.cluster_to_lba(self.root_cluster);
        let data = self.drive.read_sectors(root_lba, self.sectors_per_cluster as u8);
        if data.is_empty() {
            writer::print("[FAT] Error: Could not read root directory.\n");
            return;
        }
        
        writer::print("--- RAW DISK DUMP ---\n");

        for i in (0..data.len()).step_by(32) {
            if i + 32 > data.len() { break; }
            let entry = unsafe { &*(data.as_ptr().add(i) as *const DirectoryEntry) };

            // DEBUG: Print the first byte of every entry
            let first_byte = entry.name[0];
            let attr = entry.attr;
            
            // 0x00 = End, 0xE5 = Deleted
            if first_byte == 0x00 { 
                writer::print(&alloc::format!("[IDX {:02}] END MARKER (00)\n", i/32));
                break; 
            }
            
            // Print raw name bytes
            let name = core::str::from_utf8(&entry.name).unwrap_or("INVALID");
            
            writer::print(&alloc::format!("[IDX {:02}] {:02x} | Attr: {:02x} | Name: {}\n", 
                i/32, first_byte, attr, name));
        }
    }

    fn get_clusters(&self, start_cluster: u32) -> Vec<u32> {
        let mut clusters = Vec::new();
        let mut current = start_cluster;
        while current < 0x0FFFFFF8 && current != 0 {
            clusters.push(current);
            let fat_offset = current * 4;
            let fat_sector = self.fat_start + (fat_offset / 512);
            let sector_offset = (fat_offset % 512) as usize;
            let data = self.drive.read_sectors(fat_sector, 1);
            let next = u32::from_le_bytes(data[sector_offset..sector_offset + 4].try_into().unwrap()) & 0x0FFFFFFF;
            current = next;
        }
        clusters
    }

    pub fn read_file(&self, filename: &str) -> Option<Vec<u8>> {
        let root_lba = self.cluster_to_lba(self.root_cluster);
        let data = self.drive.read_sectors(root_lba, self.sectors_per_cluster as u8);
        if data.is_empty() { return None; }

        // 1. Find the file entry
        for i in (0..data.len()).step_by(32) {
            if i + 32 > data.len() { break; }
            let entry = unsafe { &*(data.as_ptr().add(i) as *const DirectoryEntry) };

            if entry.name[0] == 0x00 { break; }
            if entry.name[0] == 0xE5 || entry.attr == 0x0F { continue; }

            let name_str = Self::format_name(&entry.name);
            
            // Case-insensitive match
            if name_str.eq_ignore_ascii_case(filename) {
                // FOUND IT!
                let cluster = ((entry.cluster_high as u32) << 16) | (entry.cluster_low as u32);
                let size = entry.size as usize;
                
                // Read all clusters
                let clusters = self.get_clusters(cluster);
                let mut raw_data = Vec::new();
                for c in clusters {
                    let file_lba = self.cluster_to_lba(c);
                    let data = self.drive.read_sectors(file_lba, self.sectors_per_cluster as u8);
                    raw_data.extend_from_slice(&data);
                }
                
                // Trim to actual size
                if size < raw_data.len() {
                    raw_data.truncate(size);
                }
                return Some(raw_data);
            }
        }
        None
    }

    fn cluster_to_lba(&self, cluster: u32) -> u32 {
        self.partition_offset + self.data_start + ((cluster - 2) * self.sectors_per_cluster)
    }
}