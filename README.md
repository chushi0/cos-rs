> 此项目已于 2026-01-22 归档，不再维护  

# COS — Chushi Operating System

COS 是一款使用 **Rust** 编写的实验性操作系统，运行在 **x86-64** 架构上，目标是逐步实现现代操作系统的核心功能。  
目前已支持：

- 用户态多任务调度  
- 基本文件系统  
- 键盘输入与文本终端显示  
- 用户态程序与系统调用框架  

该项目主要用于学习与研究操作系统内核设计、Rust 在裸机环境中的应用。

---

## 构建与运行

在开始之前，请确保已安装：

- Rust 工具链
- `nasm`
- `qemu-system-x86_64`

执行以下命令即可从源码构建磁盘镜像并使用 QEMU 启动 COS：

```sh
cd build-scripts
cargo build
cd ..
./build-scripts/target/debug/build-scripts build
./build-scripts/target/debug/build-scripts run
````

---

## 项目结构

```
COS
├── build-scripts     # 构建磁盘镜像并启动 QEMU 的辅助工具
├── bootloader        # 引导程序（MBR + 32 位 → 64 位切换）
├── kernel            # 64 位内核主体
├── library           # 内核 / 用户态通用库
└── user              # 用户态程序与运行时支持
```

### build-scripts

用于构建磁盘镜像并通过 QEMU 运行系统。

### bootloader

* 磁盘前 512 字节的 MBR 启动代码（汇编）
* 32 位引导阶段的 Rust 代码
* 负责从实模式 / 保护模式切换到 64 位长模式并加载内核

### kernel

64 位内核实现，包含：

* 任务调度与线程管理
* 异步运行时
* 中断与系统调用
* 内存管理与设备驱动

### library

内核态与用户态共享的基础库：

* **async_io** — 异步 IO 抽象
* **async_locks** — 异步并发原语
* **elf** — ELF 文件解析与加载
* **filesystem** — 文件系统实现
* **heap** — 通用堆内存分配器
* **try_alloc** — 允许分配失败的集合与容器

### user

用户态支持库与系统程序：

#### user/library

* **cos-heap** — 用户态堆实现
* **cos-sys** — 系统调用封装
* **libc** — 简化版用户态 libc

#### user/system

* **init** — 系统初始化进程
* **shell** — 简单命令行交互进程

---

## 关键代码位置（特色实现）

以下文件包含了项目中的一些核心或代表性实现：

* `bootloader/asm/boot.s`
  MBR 启动扇区汇编代码

* `bootloader/src/bit64.rs`
  从 32 位保护模式切换到 64 位长模式

* `kernel/src/multitask/async_rt.rs`
  内核异步运行时（`spawn` / `block_on` 实现）

* `kernel/src/sync/percpu.rs`
  基于 **GS 寄存器** 的 per-CPU 数据结构

* `kernel/src/trap/syscall.rs`
  系统调用入口与分发逻辑

* `kernel/src/multitask/thread.rs`
  线程结构与调度器

* `kernel/src/memory/physics.rs`
  物理内存管理器

* `kernel/src/io/disk/ata_lba.rs`
  基于中断的 ATA LBA 磁盘驱动

* `library/filesystem/src/device/mbr.rs`
  MBR 分区表解析

* `library/filesystem/src/fs/fat32.rs`
  FAT32 文件系统（不完整实现）

* `library/heap/src/lib.rs`
  通用 Rust 堆内存分配实现

---

## 项目状态

⚠️ **本项目已于 2026-01-22 归档，不再维护。**
代码仅供学习与参考使用。

---

## License

MIT
