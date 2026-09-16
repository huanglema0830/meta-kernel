use std::path::PathBuf;

fn main() {
    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR 未设置"));
    let kernel = PathBuf::from(
        std::env::var_os("CARGO_BIN_FILE_KERNEL_kernel")
            .expect("未取到内核 ELF 路径（artifact 依赖未生效？）"),
    );

    let img = out_dir.join("boot-bios.img");
    bootloader::BiosBoot::new(&kernel)
        .create_disk_image(&img)
        .expect("创建可引导磁盘镜像失败");

    println!("cargo:rustc-env=BOOT_BIOS_IMG={}", img.display());
    println!("cargo:rerun-if-changed={}", kernel.display());
}
