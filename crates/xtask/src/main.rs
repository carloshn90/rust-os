use std::path::Path;
use std::process::{Command, exit};
use std::{env, fs};

fn main() {
    let args: Vec<String> = env::args().collect();
    let is_debug = args.iter().any(|arg| arg == "debug");
    let disk_path = "disk.img";

    println!("Building user application...");
    let build_user_status = Command::new("cargo")
    .args(["build", "-p", "user", "--target", "aarch64-unknown-none"])
    .env(
        "CARGO_TARGET_AARCH64_UNKNOWN_NONE_RUSTFLAGS",
        "-C link-arg=-Tcrates/user/user_linker.ld -C link-arg=--gc-sections -C relocation-model=static"
    )
    .status()
    .expect("Failed to run user cargo build");

    if !build_user_status.success() {
        exit(1);
    }

    println!("Extracting raw user binary...");
    let user_bin_path = "target/user_hello.bin";

    let objcopy_status = Command::new("cargo")
        .args([
            "objcopy",
            "-p",
            "user",
            "--target",
            "aarch64-unknown-none",
            "--",
            "-O",
            "binary",
            user_bin_path, // Only pass the output binary path here
        ])
        .env(
            "CARGO_TARGET_AARCH64_UNKNOWN_NONE_RUSTFLAGS",
            "-C link-arg=-Tcrates/user/user_linker.ld -C link-arg=--gc-sections -C relocation-model=static"
        )
        .status()
        .expect("Failed to run cargo objcopy on user binary");

    if !objcopy_status.success() {
        eprintln!(
            "Error: Make sure 'cargo-binutils' is installed (`cargo install cargo-binutils`)"
        );
        exit(1);
    }

    println!("Building kernel...");

    let build_kernel_status = Command::new("cargo")
        .args(["build", "-p", "kernel", "--target", "aarch64-unknown-none"])
        .env(
            "CARGO_TARGET_AARCH64_UNKNOWN_NONE_RUSTFLAGS",
            "-C link-arg=-Tcrates/kernel/linker.ld -C link-arg=--gc-sections -C relocation-model=static"
        )
        .status()
        .expect("Failed to run kernel cargo build");

    if !build_kernel_status.success() {
        exit(1);
    }

    let build_mkfs_status = Command::new("cargo")
        .args(["build", "-p", "mkfs"])
        .status()
        .expect("Failed to run kernel cargo build");

    if !build_mkfs_status.success() {
        exit(1);
    }

    println!("Creating disk image {}...", disk_path);
    if Path::new(disk_path).exists() {
        println!("Removing existing {}...", disk_path);
        fs::remove_file(disk_path).expect("Failed to remove existing disk.img");
    }

    let create_disk_status = Command::new("qemu-img")
        .args(["create", "-f", "raw", disk_path, "100M"])
        .status()
        .expect("Failed to run qemu-img create");

    if !create_disk_status.success() {
        exit(1);
    }

    println!("Running Kernel...");

    let run_mkfs_status = Command::new("cargo")
        .args(["run", "-p", "mkfs", "--", disk_path])
        .status()
        .expect("Failed to run mkfs");

    if !run_mkfs_status.success() {
        exit(1);
    }

    let mut qemu_args = vec![
        "-machine",
        "virt,virtio-mmio-transports=1,gic-version=2",
        "-cpu",
        "cortex-a53",
        "-m",
        "256M",
        "-nographic",
        "-d",
        "int",
        "-serial",
        "mon:stdio",
        "-drive",
        "if=none,file=disk.img,format=raw,id=hd0",
        "-global",
        "virtio-mmio.force-legacy=false",
        "-device",
        "virtio-blk-device,drive=hd0,bus=virtio-mmio-bus.0",
        "-kernel",
        "target/aarch64-unknown-none/debug/kernel",
    ];

    if is_debug {
        println!("Starting QEMU in debug mode (waiting for GDB connection...)");
        qemu_args.push("-S"); // Freeze CPU at startup
        qemu_args.push("-s"); // Shorthand for -gdb tcp::1234
    } else {
        println!("Starting QEMU...");
    }

    let mut binding = Command::new("qemu-system-aarch64");
    let qemu = binding.args(&qemu_args);
    println!("{:?}", qemu);
    let qemu_status = qemu.status().expect("Failed to run QEMU");

    if !qemu_status.success() {
        exit(1);
    }
}
