//! 基因库持久化（宿主侧，方案 A）。
//!
//! 内核 `GeneLibrary` 已是**零依赖纯逻辑**：`to_text()/from_text()` 负责四层编解码
//! （基础公式 / 场景公式 / 计算关系 / 验证哈希链）。本模块负责**落盘 / 读回 + 启动钩子 +
//! 轮转**，遵守内核「无 IO」红线——持久化全部在宿主。
//!
//! 存储位置由宿主决定：
//! - **native**：文件系统（默认 `<cwd>/gene_library.txt`，可用 `META_KERNEL_GENE_LIB` 覆盖完整路径）
//! - **wasm**：`localStorage`（key = `meta-kernel-genelib`）
//!
//! 设计要点：
//! - `seed_all` 幂等播种宿主需要的全部基础公式（gabor / coherence / 证据 / L4 阈值）；
//!   **改基因库即改行为**。
//! - `save_gene_library_to` 先轮转旧主文件为 `.bak`，再原子改名写入，损坏主文件可回退 `.bak`。
//! - `load_or_seed_at` 是启动钩子：有则加载，无则新建 + 播种 + 落盘。

use std::path::{Path, PathBuf};

use meta_kernel_core::gene_library::GeneLibrary;
use meta_kernel_core::l1_mapping::{seed_coherence_into, seed_gabor_into};
use meta_kernel_core::l4::threshold::{self, GOLDEN_HIGH, GOLDEN_LOW, NAME_HIGH, NAME_LOW};
use meta_kernel_core::l5_evidence;

/// 默认文件名（native）。可用环境变量 `META_KERNEL_GENE_LIB` 覆盖为完整路径。
pub fn gene_library_path() -> PathBuf {
    if let Ok(p) = std::env::var("META_KERNEL_GENE_LIB") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    PathBuf::from("gene_library.txt")
}

/// **幂等播种**：把宿主需要的所有基础公式登记进库（gabor / coherence / 证据 / L4 阈值）。
/// 缺啥补啥，已存在则 upsert 成当前缺省值——**改基因库即改行为**。
pub fn seed_all(lib: &mut GeneLibrary) {
    seed_gabor_into(lib);
    seed_coherence_into(lib);
    l5_evidence::seed_into(lib);
    threshold::seed_into(lib); // L4 黄金阈值 1.618 / 0.618 入库（基础公式层）
}

// ===== native 文件系统实现 =====

fn sibling(path: &Path, ext: &str) -> PathBuf {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "gene_library".into());
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    parent.join(format!("{name}{ext}"))
}

/// 落盘（native）：`to_text` → 写临时文件 → 原子改名；改名前把旧主文件轮转为 `.bak`（兜底回退）。
pub fn save_gene_library_to(lib: &GeneLibrary, path: &Path) -> Result<(), String> {
    let text = lib.to_text();
    let bak = sibling(path, ".bak");
    // 轮转：当前主文件 → .bak（保留上一版作回退）
    if path.exists() {
        std::fs::copy(path, &bak).map_err(|e| format!("轮转备份失败: {e}"))?;
    }
    let tmp = sibling(path, ".tmp");
    std::fs::write(&tmp, &text).map_err(|e| format!("写临时文件失败: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("原子改名失败: {e}"))?;
    Ok(())
}

/// 读回（native）：主文件优先；主文件损坏则回退 `.bak`；皆无 / 皆损 → `Err`。
pub fn load_gene_library_from(path: &Path) -> Result<GeneLibrary, String> {
    if path.exists() {
        let txt = std::fs::read_to_string(path).map_err(|e| format!("读主文件失败: {e}"))?;
        if let Some(lib) = GeneLibrary::from_text(&txt) {
            return Ok(lib);
        }
        // 主文件损坏 → 尝试 .bak
        let bak = sibling(path, ".bak");
        if bak.exists() {
            if let Ok(bt) = std::fs::read_to_string(&bak) {
                if let Some(lib) = GeneLibrary::from_text(&bt) {
                    return Ok(lib);
                }
            }
        }
        return Err("主文件解码失败且无可用回退".into());
    }
    Err("基因库文件不存在".into())
}

/// 启动钩子（native）：有则加载，无则新建 + 播种 + 落盘。
pub fn load_or_seed_at(path: &Path) -> GeneLibrary {
    match load_gene_library_from(path) {
        Ok(lib) => lib,
        Err(_) => {
            let mut lib = GeneLibrary::new();
            seed_all(&mut lib);
            let _ = save_gene_library_to(&lib, path); // 尽力持久化（无盘也不致命）
            lib
        }
    }
}

// ===== 默认路径包装（native）=====

#[cfg(not(target_arch = "wasm32"))]
pub fn save_gene_library(lib: &GeneLibrary) -> Result<(), String> {
    save_gene_library_to(lib, &gene_library_path())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn load_gene_library() -> Result<GeneLibrary, String> {
    load_gene_library_from(&gene_library_path())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn load_or_seed() -> GeneLibrary {
    load_or_seed_at(&gene_library_path())
}

// ===== wasm：localStorage =====

#[cfg(target_arch = "wasm32")]
mod wasm_storage {
    use super::*;

    const KEY: &str = "meta-kernel-genelib";

    pub fn save(lib: &GeneLibrary) -> Result<(), String> {
        let text = lib.to_text();
        let win = web_sys::window().ok_or_else(|| "无 window 上下文".to_string())?;
        let store = win
            .local_storage()
            .map_err(|_| "localStorage 不可用".to_string())?
            .ok_or_else(|| "localStorage 为 none".to_string())?;
        store
            .set_item(KEY, &text)
            .map_err(|e| format!("写入 localStorage 失败: {e:?}"))?;
        Ok(())
    }

    pub fn load() -> Result<GeneLibrary, String> {
        let win = web_sys::window().ok_or_else(|| "无 window 上下文".to_string())?;
        let store = win
            .local_storage()
            .map_err(|_| "localStorage 不可用".to_string())?
            .ok_or_else(|| "localStorage 为 none".to_string())?;
        match store.get_item(KEY).map_err(|e| format!("{e:?}"))? {
            Some(t) if !t.is_empty() => {
                GeneLibrary::from_text(&t).ok_or_else(|| "解码失败".to_string())
            }
            _ => Err("localStorage 中无基因库".to_string()),
        }
    }

    pub fn load_or_seed() -> GeneLibrary {
        match load() {
            Ok(lib) => lib,
            Err(_) => {
                let mut lib = GeneLibrary::new();
                seed_all(&mut lib);
                let _ = save(&lib);
                lib
            }
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub fn save_gene_library(lib: &GeneLibrary) -> Result<(), String> {
    wasm_storage::save(lib)
}

#[cfg(target_arch = "wasm32")]
pub fn load_gene_library() -> Result<GeneLibrary, String> {
    wasm_storage::load()
}

#[cfg(target_arch = "wasm32")]
pub fn load_or_seed() -> GeneLibrary {
    wasm_storage::load_or_seed()
}

// ===== 测试（native；由 CI 构建验证）=====

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_path(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("meta-kernel-test-{tag}-genelib.txt"))
    }

    #[test]
    fn roundtrip_save_load_restores_four_layers() {
        let p = tmp_path("roundtrip");
        let _ = std::fs::remove_file(&p);
        let _ = std::fs::remove_file(sibling(&p, ".bak"));
        let mut lib = GeneLibrary::new();
        seed_all(&mut lib);
        lib.set_base_constant("l4.threshold.high", 1.618, [1.0; 7]);
        save_gene_library_to(&lib, &p).unwrap();
        let back = load_gene_library_from(&p).expect("应能读回");
        assert_eq!(back.sizes(), lib.sizes(), "四层条目数一致");
        assert!((back.base_constant("l4.threshold.high").unwrap() - 1.618).abs() < 1e-12);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn rotation_keeps_previous_as_bak() {
        let p = tmp_path("rotate");
        let _ = std::fs::remove_file(&p);
        let _ = std::fs::remove_file(sibling(&p, ".bak"));
        let mut a = GeneLibrary::new();
        seed_all(&mut a);
        save_gene_library_to(&a, &p).unwrap();
        // 第二版：改阈值
        a.set_base_constant("l4.threshold.high", 2.5, [1.0; 7]);
        save_gene_library_to(&a, &p).unwrap();
        // 主文件应是 2.5，.bak 应是 1.618
        let main = load_gene_library_from(&p).unwrap();
        assert!(
            (main.base_constant("l4.threshold.high").unwrap() - 2.5).abs() < 1e-12,
            "主文件为新值"
        );
        let bak = load_gene_library_from(&sibling(&p, ".bak")).unwrap();
        assert!(
            (bak.base_constant("l4.threshold.high").unwrap() - 1.618).abs() < 1e-12,
            ".bak 保留旧值"
        );
        let _ = std::fs::remove_file(&p);
        let _ = std::fs::remove_file(sibling(&p, ".bak"));
    }

    #[test]
    fn load_or_seed_creates_when_missing() {
        let p = tmp_path("seed");
        let _ = std::fs::remove_file(&p);
        let _ = std::fs::remove_file(sibling(&p, ".bak"));
        let lib = load_or_seed_at(&p);
        // 应已播种 L4 + gabor + coherence + evidence 共 ≥4 条
        assert!(lib.base.len() >= 4, "启动时播种了基础公式");
        assert!(
            (lib.base_constant(NAME_HIGH).unwrap() - GOLDEN_HIGH).abs() < 1e-12,
            "L4 高阈值已入库"
        );
        assert!(
            (lib.base_constant(NAME_LOW).unwrap() - GOLDEN_LOW).abs() < 1e-12,
            "L4 低阈值已入库"
        );
        assert!(p.exists(), "已落盘");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn corrupt_main_falls_back_to_bak() {
        let p = tmp_path("corrupt");
        let _ = std::fs::remove_file(&p);
        let _ = std::fs::remove_file(sibling(&p, ".bak"));
        let mut a = GeneLibrary::new();
        seed_all(&mut a);
        save_gene_library_to(&a, &p).unwrap();
        // 破坏主文件
        std::fs::write(&p, "not a genelib\n").unwrap();
        let back = load_gene_library_from(&p).expect("应从 .bak 回退");
        assert_eq!(back.sizes(), a.sizes());
        let _ = std::fs::remove_file(&p);
        let _ = std::fs::remove_file(sibling(&p, ".bak"));
    }
}
