#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct MemoryRegion {
    base_addr: u64,
    length: u64,
    region_type: u32,
}

impl MemoryRegion {
    const TYPE_USABLE: u32 = 1;
    const TYPE_RESERVED: u32 = 2;
}

/// 规整内存
///
/// BIOS给出的内存信息可能并不符合我们的预期，例如可能存在重叠、乱序、无法识别的类型等。
/// 我们需要对其进行整理。
pub fn normalize_memory_region(mut memory_region: &mut [MemoryRegion]) -> &mut [MemoryRegion] {
    // 1. 排序
    memory_region.sort_unstable_by_key(|memory_region| memory_region.base_addr);

    // 2. 将未知类型改为不可用
    memory_region
        .iter_mut()
        .filter(|region| region.region_type != MemoryRegion::TYPE_USABLE)
        .for_each(|region| region.region_type = MemoryRegion::TYPE_RESERVED);

    loop {
        // 3. 解决重叠问题
        let mut change_base_addr = false;
        for i in 1..memory_region.len() {
            let prev_base_addr = memory_region[i - 1].base_addr;
            let prev_length = memory_region[i - 1].length;
            let prev_end_addr = prev_base_addr + prev_length;
            let cur_base_addr = memory_region[i].base_addr;
            if prev_end_addr <= cur_base_addr {
                continue;
            }

            // 发现重叠，重叠部分视为更严格的限制，如果这两个块有一个不可用，则将重叠部分视为不可用
            let overlap = prev_end_addr - cur_base_addr;
            if memory_region[i - 1].region_type == MemoryRegion::TYPE_RESERVED {
                // 前一个块不可用，减小当前块
                memory_region[i].base_addr += overlap;
                memory_region[i].length = memory_region[i].length.saturating_sub(overlap);
                change_base_addr = true;
                continue;
            }

            // 前一个块可用，减小前一个块
            memory_region[i - 1].length = memory_region[i - 1].length.saturating_sub(overlap);
        }

        if change_base_addr {
            // 重排序
            memory_region.sort_unstable_by_key(|memory_region| memory_region.base_addr);
            continue;
        }

        // 4. 移除0长度结构体
        let mut len = 0;
        for i in 0..memory_region.len() {
            if memory_region[i].length > 0 {
                memory_region[len] = memory_region[i];
                len += 1;
            }
        }

        // 如果没有移除任何结构体，继续下一步
        // 如果移除了，则要重新检查是否重叠
        if len == memory_region.len() {
            break;
        }
        memory_region = &mut memory_region[..len];
    }

    // 5. 合并相邻的同类型内存
    let mut len = memory_region.len();
    let mut i = 1;
    while i < len {
        if memory_region[i - 1].region_type != memory_region[i].region_type {
            i += 1;
            continue;
        }

        let prev_base_addr = memory_region[i - 1].base_addr;
        let prev_length = memory_region[i - 1].length;
        let prev_end_addr = prev_base_addr + prev_length;
        let cur_base_addr = memory_region[i].base_addr;
        if prev_end_addr != cur_base_addr {
            i += 1;
            continue;
        }

        // 合并
        memory_region[i - 1].length += memory_region[i].length;
        for j in (i + 1)..len {
            memory_region[j - 1] = memory_region[j];
        }
        len -= 1;
    }

    &mut memory_region[..len]
}
