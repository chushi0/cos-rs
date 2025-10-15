#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct MemoryRegion {
    base_addr: u64,
    length: u64,
    region_type: u32,
}
