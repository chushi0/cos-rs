//! 编译COS项目的工具代码
//!
//! 此crate在宿主机环境上运行，不会打包到产物中，因此此项目无需#![no_std]

use std::{
    fs::{self, File},
    io::Read,
    path::PathBuf,
    pin::pin,
    process::{Command, Stdio},
    str::FromStr,
    sync::Arc,
    task::{Context, Poll, Waker},
};

use clap::Parser;
use filesystem::{
    device::{
        BlockDevice,
        mbr::{
            MbrPartitionDevice, MbrPartitionEntry, PARTITION_TYPE_BOOTLOADER, PARTITION_TYPE_FAT32,
        },
    },
    fs::{FileSystem, fat32::Fat32FileSystem},
};

use crate::adapter::HostFileBlockDevice;

mod adapter;

const KERNEL_DISK_SIZE: u64 = 1024 * 1024 * 10; // 10M

#[derive(clap::Parser)]
enum BuildArgs {
    /// 完整编译项目
    Build {
        /// 以debug模式编译内核，附带符号表
        #[arg(long)]
        debug: bool,
    },
    /// 运行项目
    Run {
        /// qemu附加-S -s -no-reboot、-no-shutdown参数以便调试
        #[arg(long)]
        debug: bool,
    },
}

const SYSTEM_APPLICATIONS: &[&str] = &["init", "shell"];

fn main() {
    let arg = BuildArgs::parse();

    match arg {
        BuildArgs::Build { debug } => build(debug),
        BuildArgs::Run { debug } => run(debug),
    }
}

fn build(debug: bool) {
    fs::create_dir_all("build").expect("failed to create build cache dir");
    compile_boot_asm();
    compile_loader();
    extract_loader_binary();
    compile_kernel(debug);
    extract_kernel_binary(debug);
    compile_system_application();
    build_image();
}

fn run(debug: bool) {
    let mut cmd = Command::new("qemu-system-x86_64");
    cmd.args(["-drive", "format=raw,file=./build/disk.img"]);
    if debug {
        cmd.arg("-S")
            .arg("-s")
            .arg("-no-reboot")
            .arg("-no-shutdown");
    }

    cmd.stdin(Stdio::inherit());
    cmd.stdout(Stdio::inherit());
    cmd.stderr(Stdio::inherit());

    let mut child = cmd
        .spawn()
        .expect("failed to start qemu-system-x86_64. may qemu is not installed?");

    let status = child.wait().expect("failed to start qemu");
    if !status.success() {
        panic!("qemu is stopped with non-zero status: {status}")
    }
}

fn compile_boot_asm() {
    let mut cmd = Command::new("nasm");
    cmd.arg("-f").arg("bin");
    cmd.arg("./bootloader/asm/boot.s");
    cmd.arg("-o").arg("./build/boot.bin");

    cmd.stdin(Stdio::inherit());
    cmd.stdout(Stdio::inherit());
    cmd.stderr(Stdio::inherit());

    let mut child = cmd
        .spawn()
        .expect("failed to compile boot.s. may nasm is not installed?");

    let status = child.wait().expect("failed to compile bootloader");
    if !status.success() {
        panic!("boot.s compile failed: nasm exit with code `{status}`");
    }
}

fn compile_loader() {
    let mut cmd = Command::new("cargo");
    cmd.arg("build").arg("--release");
    cmd.current_dir(
        PathBuf::from_str("./bootloader")
            .unwrap()
            .canonicalize()
            .unwrap(),
    );
    cmd.stdin(Stdio::inherit());
    cmd.stdout(Stdio::inherit());
    cmd.stderr(Stdio::inherit());
    let mut child = cmd.spawn().expect("failed to build bootloader");
    let status = child.wait().expect("failed to build bootloader");
    if !status.success() {
        panic!("failed to build bootloader: cargo exit with non-zero status: {status}")
    }
}

fn compile_kernel(debug: bool) {
    let mut cmd = Command::new("cargo");
    cmd.arg("build");
    if !debug {
        cmd.arg("--release");
    }
    cmd.current_dir(
        PathBuf::from_str("./kernel")
            .unwrap()
            .canonicalize()
            .unwrap(),
    );
    cmd.stdin(Stdio::inherit());
    cmd.stdout(Stdio::inherit());
    cmd.stderr(Stdio::inherit());
    let mut child = cmd.spawn().expect("failed to build kernel");
    let status = child.wait().expect("failed to build kernel");
    if !status.success() {
        panic!("failed to build kernel: cargo exit with non-zero status: {status}")
    }
}

fn compile_system_application() {
    let mut cmd = Command::new("cargo");
    cmd.arg("build").arg("--release");
    cmd.current_dir(
        PathBuf::from_str("./user/system")
            .unwrap()
            .canonicalize()
            .unwrap(),
    );
    cmd.stdin(Stdio::inherit());
    cmd.stdout(Stdio::inherit());
    cmd.stderr(Stdio::inherit());
    let mut child = cmd.spawn().expect("failed to build system application");
    let status = child.wait().expect("failed to build system application");
    if !status.success() {
        panic!("failed to build system application: cargo exit with non-zero status: {status}")
    }
}

fn extract_loader_binary() {
    let mut cmd = Command::new("rust-objcopy");
    cmd.arg("./target/i386-unknown-none/release/bootloader")
        .arg("-O")
        .arg("binary")
        .arg("--gap-fill")
        .arg("0x00")
        .arg("./../build/loader.bin");
    cmd.current_dir(
        PathBuf::from_str("./bootloader")
            .unwrap()
            .canonicalize()
            .unwrap(),
    );
    cmd.stdin(Stdio::inherit());
    cmd.stdout(Stdio::inherit());
    cmd.stderr(Stdio::inherit());
    let mut child = cmd
        .spawn()
        .expect("failed to extract loader binary. may rust-objcopy is not installed?");
    let status = child.wait().expect("failed to extract loader binary");
    if !status.success() {
        panic!("failed to extract loader binary: rust-objcopy exit with non-zero status: {status}")
    }
}

fn extract_kernel_binary(debug: bool) {
    let elf_path = if debug {
        "./target/x86_64-unknown-cos/debug/kernel"
    } else {
        "./target/x86_64-unknown-cos/release/kernel"
    };
    let mut cmd = Command::new("rust-objcopy");
    cmd.arg(elf_path)
        .arg("-O")
        .arg("binary")
        .arg("--gap-fill")
        .arg("0x00")
        .arg("./../build/kernel.bin");
    cmd.current_dir(
        PathBuf::from_str("./kernel")
            .unwrap()
            .canonicalize()
            .unwrap(),
    );
    cmd.stdin(Stdio::inherit());
    cmd.stdout(Stdio::inherit());
    cmd.stderr(Stdio::inherit());
    let mut child = cmd
        .spawn()
        .expect("failed to extract kernel binary. may rust-objcopy is not installed?");
    let status = child.wait().expect("failed to extract kernel binary");
    if !status.success() {
        panic!("failed to extract kernel binary: rust-objcopy exit with non-zero status: {status}")
    }
}

fn build_image() {
    let boot = fs::read("./build/boot.bin").expect("failed to read ./build/boot.bin");
    let mut loader = fs::read("./build/loader.bin").expect("failed to read ./build/loader.bin");
    let mut kernel = fs::read("./build/kernel.bin").expect("failed to read ./build/kernel.bin");

    pad_to_fam(&mut loader);
    pad_to_fam(&mut kernel);

    let loader_size = calc_fam_size(loader.len(), u8::MAX as usize);
    let kernel_size = calc_fam_size(kernel.len(), u16::MAX as usize);

    let disk = HostFileBlockDevice::new("./build/disk.img", KERNEL_DISK_SIZE)
        .expect("failed to create ./build/disk.img");

    block_on(disk.write_block(0, &boot)).expect("failed to write mbr boot for disk.img");

    let mut mbr = block_on(MbrPartitionDevice::format(
        disk,
        [
            Some(MbrPartitionEntry {
                bootable: true,
                start: 1,
                end: loader_size + 1,
                partition_type: PARTITION_TYPE_BOOTLOADER,
            }),
            Some(MbrPartitionEntry {
                bootable: false,
                start: loader_size + 1,
                end: loader_size + kernel_size + 1,
                partition_type: PARTITION_TYPE_BOOTLOADER,
            }),
            Some(MbrPartitionEntry {
                bootable: false,
                start: loader_size + kernel_size + 1,
                end: (KERNEL_DISK_SIZE / 512) as u32,
                partition_type: PARTITION_TYPE_FAT32,
            }),
            None,
        ],
    ))
    .expect("failed to mbr format disk.img");

    let loader_partition = mbr[0]
        .take()
        .expect("codebug: block device should not be none since we format it already (loader)");
    block_on(loader_partition.write_blocks(0, loader_size as u64, &loader))
        .expect("failed to write loader partition to disk.img");

    let kernel_partition = mbr[1]
        .take()
        .expect("codebug: block device should not be none since we format it already (kernel)");
    block_on(kernel_partition.write_blocks(0, kernel_size as u64, &kernel))
        .expect("failed to write kernel partition to disk.img");

    let file_system_partition = mbr[2].take().expect(
        "codebug: block device should not be none since we format it already (file_system)",
    );
    let fs = block_on(Fat32FileSystem::with_format(Arc::new(
        file_system_partition,
    )))
    .expect("failed to format file system for disk.img");

    let system_application_dir = filesystem::path::PathBuf::from_str("/system")
        .expect("codebug: failed to create system application path");
    block_on(fs.create_directory(system_application_dir.as_path()))
        .expect("failed to create system application path");

    for system_application in SYSTEM_APPLICATIONS {
        let mut filepath = system_application_dir.clone();
        filepath.extends(
            &filesystem::path::PathBuf::from_str(&system_application)
                .expect("failed to create system application path"),
        );
        block_on(fs.create_file(filepath.as_path())).expect("failed to create file");
        let mut file = block_on(fs.open_file(filepath.as_path())).expect("failed to open file");

        let mut host_file = File::open(format!(
            "./user/system/target/x86_64-unknown-cos/release/{system_application}"
        ))
        .expect("failed to open host file");
        let mut buf = [0u8; 8192];
        loop {
            let len = host_file
                .read(&mut buf)
                .expect("failed to read from host file");
            if len == 0 {
                break;
            }
            block_on(file.write(&buf[..len])).expect("failed to write to file");
        }
        block_on(file.close()).expect("failed to close file");
    }

    let welcome_path = filesystem::path::PathBuf::from_str("/system/welcome.txt")
        .expect("failed to create welcome path");
    block_on(fs.create_file(welcome_path.as_path())).expect("failed to create file");
    let mut file = block_on(fs.open_file(welcome_path.as_path())).expect("failed to open file");
    let welcome = b"Welcome to COS shell!\nThis welcome message is from /system/welcome.txt!\n";
    block_on(file.write(welcome)).expect("failed to write to file");
    block_on(file.close()).expect("failed to close file");

    block_on(fs.unmount()).expect("failed to unmount formatted file system for disk.img");
}

fn pad_to_fam(binary: &mut Vec<u8>) {
    let len = binary.len();
    let remain = len % 512;
    if remain > 0 {
        binary.resize(len + 512 - remain, 0);
    }
}

fn calc_fam_size(size: usize, max: usize) -> u32 {
    assert!(size <= max * 512, "code is too large");

    ((size + 512 - 1) / 512) as u32
}

fn block_on<F: Future>(f: F) -> F::Output {
    let waker = Waker::noop();
    let mut ctx = Context::from_waker(&waker);
    let mut f = pin!(f);
    loop {
        match f.as_mut().poll(&mut ctx) {
            Poll::Ready(v) => return v,
            Poll::Pending => {}
        }
    }
}
