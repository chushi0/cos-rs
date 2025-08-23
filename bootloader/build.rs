use std::{fs, path::Path, process::Command};

fn main() {
    let output = std::env::var("OUT_DIR").unwrap();
    fs::create_dir_all(&output).unwrap();
    let mut cmd = Command::new("nasm");
    cmd.arg("-f")
        .arg("bin")
        .arg("./asm/stub.s")
        .arg("-o")
        .arg(Path::new(&output).join("stub.bin"));
    let output = cmd.output().unwrap();
    if !output.status.success() {
        panic!(
            "failed to build stub.asm.\n  output: {}\n  stderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
