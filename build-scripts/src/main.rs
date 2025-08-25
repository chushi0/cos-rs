//! 编译COS项目的工具代码
//!
//! 此crate在宿主机环境上运行，不会打包到产物中，因此此项目无需#![no_std]

use std::{
    fs::{self, File},
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
    str::FromStr,
};

use clap::Parser;

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

fn extract_loader_binary() {
    let mut cmd = Command::new("rust-objcopy");
    cmd.arg("./target/i386-unknown-none/release/bootloader")
        .arg("-O")
        .arg("binary")
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
    let mut boot = fs::read("./build/boot.bin").expect("failed to read ./build/boot.bin");
    let mut loader = fs::read("./build/loader.bin").expect("failed to read ./build/loader.bin");
    let mut kernel = fs::read("./build/kernel.bin").expect("failed to read ./build/kernel.bin");

    pad_to_fam(&mut loader);
    pad_to_fam(&mut kernel);

    let loader_size = calc_fam_size(loader.len(), u8::MAX as usize) as u8;
    let kernel_size = calc_fam_size(kernel.len(), u16::MAX as usize) as u16;
    boot[507] = loader_size;
    boot[508] = (kernel_size & 0xff) as u8;
    boot[509] = ((kernel_size & 0xff00) >> 8) as u8;

    let mut disk = File::create("./build/disk.img").expect("failed to create ./build/disk.img");
    disk.write_all(&boot)
        .expect("failed to write boot to disk.img");
    disk.write_all(&loader)
        .expect("failed to write loader to disk.img");
    disk.write_all(&kernel)
        .expect("failed to write kernel to disk.img");
}

fn pad_to_fam(binary: &mut Vec<u8>) {
    let len = binary.len();
    let remain = len % 512;
    if remain > 0 {
        binary.resize(len + 512 - remain, 0);
    }
}

fn calc_fam_size(size: usize, max: usize) -> usize {
    assert!(size <= max * 512, "code is too large");

    (size + 512 - 1) / 512
}
