#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct MemoryRegion {
    pub base_addr: u64,
    pub length: u64,
    pub region_type: u32,
}

const _: () = {
    assert!(size_of::<MemoryRegion>() == 20);
};
