//! 打包器（host 侧）：把内核 ELF 与 bootloader 0.11 合成可引导磁盘镜像。
//! 真正的镜像生成在 `build.rs`，本 bin 仅作为包的 target 存在。
fn main() {
    println!("boot image: {}", env!("BOOT_BIOS_IMG"));
}
