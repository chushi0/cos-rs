pub mod page;

pub(self) mod heap;
pub(self) mod physics;

pub unsafe fn init(memory_region: &'static [crate::bootloader::MemoryRegion]) {
    unsafe {
        // 页表首先初始化，我们需要接手bootloader设置的页表，
        // 并据此推算内核占用内存大小
        page::init();
        physics::init(memory_region);
    }
}
