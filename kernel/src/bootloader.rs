#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct MemoryRegion {
    pub base_addr: u64,
    pub length: u64,
    pub region_type: u32,
}
