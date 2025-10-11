use std::env;
use std::path::PathBuf;

fn main() {
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").unwrap());

    let kernel_path = PathBuf::from(
        env::var_os("CARGO_BIN_FILE_KERNEL_kernel")
            .expect("kernel artifact env var not found; check bin name"),
    );

    let uefi_img = out_dir.join("uefi.img");
    bootloader::UefiBoot::new(&kernel_path)
        .create_disk_image(&uefi_img)
        .expect("failed to build UEFI disk image");

    println!("cargo:rerun-if-changed={}", kernel_path.display());

    println!("cargo:rustc-env=UEFI_IMG={}", uefi_img.display());
}
