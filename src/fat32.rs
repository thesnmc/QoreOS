#[repr(C, packed)]
pub struct Fat32BootSector {
    pub jmp_boot: [u8; 3],
    pub oem_name: [u8; 8],
    pub bytes_per_sector: u16,
    pub sectors_per_cluster: u8,
    pub reserved_sector_count: u16,
    pub num_fats: u8,
    pub root_entry_count: u16,
    pub total_sectors_16: u16,
    pub media: u8,
    pub fat_size_16: u16,
    pub sectors_per_track: u16,
    pub num_heads: u16,
    pub hidden_sectors: u32,
    pub total_sectors_32: u32,
    // FAT32 Extended fields
    pub fat_size_32: u32,
    pub ext_flags: u16,
    pub fs_version: u16,
    pub root_cluster: u32,
    pub fs_info: u16,
    pub backup_boot_sector: u16,
    pub reserved_0: [u8; 12],
    pub drive_number: u8,
    pub reserved_1: u8,
    pub boot_signature: u8,
    pub volume_id: u32,
    pub volume_label: [u8; 11],
    pub fs_type: [u8; 8],
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct Fat32DirEntry {
    pub name: [u8; 11],
    pub attr: u8,
    pub nt_res: u8,
    pub crt_time_tenth: u8,
    pub crt_time: u16,
    pub crt_date: u16,
    pub lst_acc_date: u16,
    pub fst_clus_hi: u16,
    pub wrt_time: u16,
    pub wrt_date: u16,
    pub fst_clus_lo: u16,
    pub file_size: u32,
}

// --- NEW ABSTRACTION LAYER ---

pub struct Fat32Volume {
    pub first_data_sector: u32,
    pub sectors_per_cluster: u32,
    pub root_cluster: u32,
    pub fat_start_sector: u32,
}

impl Fat32Volume {
    /// Parses the raw Boot Sector and caches the critical offsets.
    pub fn new(bpb: &Fat32BootSector) -> Self {
        let fat_start_sector = bpb.reserved_sector_count as u32;
        let first_data_sector = fat_start_sector + (bpb.num_fats as u32 * bpb.fat_size_32);
        
        Fat32Volume {
            first_data_sector,
            sectors_per_cluster: bpb.sectors_per_cluster as u32,
            root_cluster: bpb.root_cluster,
            fat_start_sector,
        }
    }

    /// Converts a FAT32 Cluster Number into an absolute LBA (Logical Block Address) for the NVMe driver.
    pub fn cluster_to_lba(&self, cluster: u32) -> u64 {
        if cluster < 2 { return 0; } // Clusters 0 and 1 are reserved in FAT32
        (self.first_data_sector + ((cluster - 2) * self.sectors_per_cluster)) as u64
    }
}