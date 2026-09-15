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
use meta_kernel_core::habit::HabitPool;
use meta_kernel_core::l1_mapping::{seed_coherence_into, seed_gabor_into};
use meta_kernel_core::l4::threshold::{self, GOLDEN_HIGH, GOLDEN_LOW, NAME_HIGH, NAME_LOW};
use meta_kernel_core::l5_evidence;
use meta_kernel_core::l5_quad::Quad;
use meta_kernel_core::trace::TraceStore;

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

// ===== 痕迹 / 习气 / 四元组持久化（v0.125）=====
//
// 分工与基因库一致：**序列化在内核（`to_text`/`from_text`，零依赖），文件 IO 在宿主**。
// 落盘同样走「轮转 `.bak` → 临时文件 → 原子改名」，读回时主文件损坏可回退 `.bak`。

/// 痕迹存储容量（反序列化上限；与 `TraceStore::default()` 一致）。
pub const TRACE_CAP: usize = 2048;

/// 四元组持久化快照：**基线** + **每个标签**的四元组。
#[derive(Debug, Clone, PartialEq)]
pub struct QuadState {
    /// 四元组基线（紧张/平静/喜欢/安全）。
    pub baseline: [f64; 4],
    /// 各标签当前四元组（按标签顺序）。
    pub tabs: Vec<[f64; 4]>,
}

impl QuadState {
    /// 序列化：首行 `b <四元组>`，其后每个标签一行 `t <四元组>`。
    pub fn to_text(&self) -> String {
        let mut s = String::new();
        s.push_str("b ");
        s.push_str(&Quad::from_array(self.baseline).to_text());
        s.push('\n');
        for t in &self.tabs {
            s.push_str("t ");
            s.push_str(&Quad::from_array(*t).to_text());
            s.push('\n');
        }
        s
    }
    /// 反序列化（`b` 行缺失 → `None`；任一 `t` 行非法 → 整份拒绝，避免半截状态）。
    pub fn from_text(text: &str) -> Option<Self> {
        let mut baseline: Option<[f64; 4]> = None;
        let mut tabs: Vec<[f64; 4]> = Vec::new();
        for line in text.lines() {
            let l = line.trim();
            if l.is_empty() {
                continue;
            }
            if let Some(v) = l.strip_prefix("b ") {
                baseline = Some(Quad::from_text(v.trim())?.to_array());
            } else if let Some(v) = l.strip_prefix("t ") {
                tabs.push(Quad::from_text(v.trim())?.to_array());
            }
        }
        let baseline = baseline?;
        Some(QuadState { baseline, tabs })
    }
}

/// 痕迹文件默认路径（native）。可用 `META_KERNEL_TRACES` 覆盖为完整路径。
pub fn trace_store_path() -> PathBuf {
    if let Ok(p) = std::env::var("META_KERNEL_TRACES") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    PathBuf::from("trace_store.txt")
}

/// 习气文件默认路径（native）。可用 `META_KERNEL_HABITS` 覆盖为完整路径。
pub fn habit_pool_path() -> PathBuf {
    if let Ok(p) = std::env::var("META_KERNEL_HABITS") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    PathBuf::from("habit_pool.txt")
}

/// 四元组文件默认路径（native）。可用 `META_KERNEL_QUAD` 覆盖为完整路径。
pub fn quad_state_path() -> PathBuf {
    if let Ok(p) = std::env::var("META_KERNEL_QUAD") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    PathBuf::from("quad_state.txt")
}

/// 落盘文本（native）：写临时文件 → 原子改名；改名前把旧主文件轮转为 `.bak`。
fn save_text_to(text: &str, path: &Path) -> Result<(), String> {
    let bak = sibling(path, ".bak");
    if path.exists() {
        std::fs::copy(path, &bak).map_err(|e| format!("轮转备份失败: {e}"))?;
    }
    let tmp = sibling(path, ".tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("写临时文件失败: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("原子改名失败: {e}"))?;
    Ok(())
}

/// 读回并解码：**主文件优先，主文件损坏回退 `.bak`**，皆无 / 皆损 → `Err`。
fn load_decoded<T>(path: &Path, decode: impl Fn(&str) -> Option<T>) -> Result<T, String> {
    if path.exists() {
        let txt = std::fs::read_to_string(path).map_err(|e| format!("读主文件失败: {e}"))?;
        if let Some(v) = decode(&txt) {
            return Ok(v);
        }
    }
    let bak = sibling(path, ".bak");
    if bak.exists() {
        let bt = std::fs::read_to_string(&bak).map_err(|e| format!("读回退文件失败: {e}"))?;
        if let Some(v) = decode(&bt) {
            return Ok(v);
        }
    }
    Err(if path.exists() {
        "主文件解码失败且无可用回退".into()
    } else {
        "文件不存在".into()
    })
}

/// 痕迹解码（空文件 → 空存储；**非空却零条目**视为损坏，交由 `.bak` 回退）。
fn decode_traces(t: &str) -> Option<TraceStore> {
    let st = TraceStore::from_text(t, TRACE_CAP);
    if !t.trim().is_empty() && st.is_empty() {
        None
    } else {
        Some(st)
    }
}

/// 习气解码（空文件 → 空池；**非空却零条目**视为损坏）。
fn decode_habits(t: &str) -> Option<HabitPool> {
    let hp = HabitPool::from_text(t);
    if !t.trim().is_empty() && hp.is_empty() {
        None
    } else {
        Some(hp)
    }
}

// ---- 指定路径版（验收/隔离测试用）----

pub fn save_traces_to(s: &TraceStore, path: &Path) -> Result<(), String> {
    save_text_to(&s.to_text(), path)
}
pub fn load_traces_from(path: &Path) -> Result<TraceStore, String> {
    load_decoded(path, decode_traces)
}

pub fn save_habits_to(p: &HabitPool, path: &Path) -> Result<(), String> {
    save_text_to(&p.to_text(), path)
}
pub fn load_habits_from(path: &Path) -> Result<HabitPool, String> {
    load_decoded(path, decode_habits)
}

pub fn save_quad_to(q: &QuadState, path: &Path) -> Result<(), String> {
    save_text_to(&q.to_text(), path)
}
pub fn load_quad_from(path: &Path) -> Result<QuadState, String> {
    load_decoded(path, QuadState::from_text)
}

// ---- 默认路径包装（native）----

#[cfg(not(target_arch = "wasm32"))]
pub fn save_traces(s: &TraceStore) -> Result<(), String> {
    save_traces_to(s, &trace_store_path())
}
#[cfg(not(target_arch = "wasm32"))]
pub fn load_traces() -> Result<TraceStore, String> {
    load_traces_from(&trace_store_path())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn save_habits(p: &HabitPool) -> Result<(), String> {
    save_habits_to(p, &habit_pool_path())
}
#[cfg(not(target_arch = "wasm32"))]
pub fn load_habits() -> Result<HabitPool, String> {
    load_habits_from(&habit_pool_path())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn save_quad(q: &QuadState) -> Result<(), String> {
    save_quad_to(q, &quad_state_path())
}
#[cfg(not(target_arch = "wasm32"))]
pub fn load_quad() -> Result<QuadState, String> {
    load_quad_from(&quad_state_path())
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

    /// 通用文本写入（痕迹/习气/四元组与基因库共用同一通道）。
    pub fn save_text(key: &str, text: &str) -> Result<(), String> {
        let win = web_sys::window().ok_or_else(|| "无 window 上下文".to_string())?;
        let store = win
            .local_storage()
            .map_err(|_| "localStorage 不可用".to_string())?
            .ok_or_else(|| "localStorage 为 none".to_string())?;
        store
            .set_item(key, text)
            .map_err(|e| format!("写入 localStorage 失败: {e:?}"))?;
        Ok(())
    }

    /// 通用文本读取。
    pub fn load_text(key: &str) -> Result<String, String> {
        let win = web_sys::window().ok_or_else(|| "无 window 上下文".to_string())?;
        let store = win
            .local_storage()
            .map_err(|_| "localStorage 不可用".to_string())?
            .ok_or_else(|| "localStorage 为 none".to_string())?;
        match store.get_item(key).map_err(|e| format!("{e:?}"))? {
            Some(t) if !t.is_empty() => Ok(t),
            _ => Err("localStorage 中无该键".to_string()),
        }
    }

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

// ---- 默认通道包装（wasm：localStorage）----

#[cfg(target_arch = "wasm32")]
pub fn save_traces(s: &TraceStore) -> Result<(), String> {
    wasm_storage::save_text("meta-kernel-traces", &s.to_text())
}
#[cfg(target_arch = "wasm32")]
pub fn load_traces() -> Result<TraceStore, String> {
    decode_traces(&wasm_storage::load_text("meta-kernel-traces")?).ok_or_else(|| "痕迹解码失败".into())
}

#[cfg(target_arch = "wasm32")]
pub fn save_habits(p: &HabitPool) -> Result<(), String> {
    wasm_storage::save_text("meta-kernel-habits", &p.to_text())
}
#[cfg(target_arch = "wasm32")]
pub fn load_habits() -> Result<HabitPool, String> {
    decode_habits(&wasm_storage::load_text("meta-kernel-habits")?).ok_or_else(|| "习气解码失败".into())
}

#[cfg(target_arch = "wasm32")]
pub fn save_quad(q: &QuadState) -> Result<(), String> {
    wasm_storage::save_text("meta-kernel-quad", &q.to_text())
}
#[cfg(target_arch = "wasm32")]
pub fn load_quad() -> Result<QuadState, String> {
    QuadState::from_text(&wasm_storage::load_text("meta-kernel-quad")?).ok_or_else(|| "四元组解码失败".into())
}

// ===== 测试（native；由 CI 构建验证）=====

#[cfg(test)]
mod tests {
    use super::*;
    use meta_kernel_core::trace::{Trace, TraceType};

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

    // ===== v0.125：痕迹 / 习气 / 四元组持久化 =====

    fn mk_trace(step: u64, tt: TraceType, fp: u64, intensity: f32) -> Trace {
        Trace { step, intensity, trace_type: tt, fingerprint: fp, energy_flow: 0.5 }
    }

    fn clean(p: &Path) {
        let _ = std::fs::remove_file(p);
        let _ = std::fs::remove_file(sibling(p, ".bak"));
    }

    #[test]
    fn traces_roundtrip_save_load_restores_entries() {
        let p = tmp_path("traces");
        clean(&p);
        let mut s = TraceStore::new();
        for i in 1..=6u64 {
            let tt = if i % 2 == 0 { TraceType::Fire } else { TraceType::Wind };
            s.record(mk_trace(i, tt, 100 + i, 0.4));
        }
        save_traces_to(&s, &p).unwrap();
        let back = load_traces_from(&p).expect("应能读回痕迹");
        // 真实语义：条数 / 指纹序列 / 类型分布 逐项一致（不只比"非空"）
        assert_eq!(back.len(), s.len(), "条数一致");
        let fps: Vec<u64> = back.all().iter().map(|t| t.fingerprint).collect();
        let want: Vec<u64> = s.all().iter().map(|t| t.fingerprint).collect();
        assert_eq!(fps, want, "指纹序列一致");
        assert_eq!(back.counts_by_type(), s.counts_by_type(), "类型分布一致");
        clean(&p);
    }

    #[test]
    fn habits_roundtrip_save_load_restores_strength() {
        let p = tmp_path("habits");
        clean(&p);
        let mut pool = HabitPool::new();
        for i in 1..=3u64 {
            pool.observe(&mk_trace(i, TraceType::Wind, 1, 0.4));
        }
        for i in 1..=12u64 {
            pool.observe(&mk_trace(100 + i, TraceType::Earth, 2, 0.9));
        }
        save_habits_to(&pool, &p).unwrap();
        let back = load_habits_from(&p).expect("应能读回习气");
        assert_eq!(back.len(), pool.len(), "习气条数一致");
        assert_eq!(back.strongest().unwrap().fingerprint, 2, "重启后最强习气仍是指纹 2");
        let a = back.get(2).expect("存在");
        let b = pool.get(2).expect("存在");
        assert!((a.strength - b.strength).abs() < 1e-6, "强度恢复 {} vs {}", a.strength, b.strength);
        assert_eq!(a.count, b.count, "出现次数恢复");
        clean(&p);
    }

    #[test]
    fn quad_state_roundtrip_save_load_restores_four_values() {
        let p = tmp_path("quad");
        clean(&p);
        let qs = QuadState {
            baseline: [0.2, 0.6, 0.5, 0.6],
            tabs: vec![[0.1, 0.7, 0.4, 0.8], [0.9, 0.2, 0.3, 0.4], [0.31, 0.62, 0.48, 0.75]],
        };
        save_quad_to(&qs, &p).unwrap();
        let back = load_quad_from(&p).expect("应能读回四元组");
        assert_eq!(back, qs, "基线 + 各标签四元组逐项一致");
        clean(&p);
    }

    #[test]
    fn missing_state_files_return_err() {
        let d = std::env::temp_dir().join("meta-kernel-test-missing-state");
        let _ = std::fs::create_dir_all(&d);
        let tp = d.join("trace_store.txt");
        let hp = d.join("habit_pool.txt");
        let qp = d.join("quad_state.txt");
        for f in [&tp, &hp, &qp] {
            clean(f);
        }
        assert!(load_traces_from(&tp).is_err(), "痕迹文件缺失 → Err");
        assert!(load_habits_from(&hp).is_err(), "习气文件缺失 → Err");
        assert!(load_quad_from(&qp).is_err(), "四元组文件缺失 → Err");
    }

    #[test]
    fn corrupt_traces_falls_back_to_bak() {
        let p = tmp_path("traces-corrupt");
        clean(&p);
        let mut s = TraceStore::new();
        s.record(mk_trace(1, TraceType::Fire, 55, 0.6));
        save_traces_to(&s, &p).unwrap();
        // 破坏主文件（非空但零有效条目 → 应回退 .bak）
        std::fs::write(&p, "total garbage\nnot a trace\n").unwrap();
        let back = load_traces_from(&p).expect("应从 .bak 回退");
        assert_eq!(back.len(), 1, "回退后仍有 1 条");
        assert_eq!(back.all()[0].fingerprint, 55);
        clean(&p);
    }
}
