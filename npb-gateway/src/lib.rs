//! # NPB 网关（L3 应用层接入口，见 docs/API_GATEWAY_DESIGN.md v1.0）
//!
//! 铁律（与设计文档一致）：
//! - **投影**：对外读数全部来自 npb FFI 直读；网关不维护"第二业务状态"（唯一例外：
//!   SSE 跨线程转发所需的只读投影快照，其源=内核逐 tick 读数，每次操作后刷新，无漂移）；
//! - **内核零改动**：本 crate 只依赖 npb（rlib），不改 meta-kernel-core/npb 源码；
//! - **单一写者**：内核访问集中在 `kern` 工作线程（npb 内核为 thread_local，多线程各自
//!   持空内核；故 push/读全部经 mpsc 串行到单线程执行 —— 同时天然满足协议"单一写者"语义）；
//! - **不空转**：内核 tick 仅由外部 `push` 驱动，网关不伪造输入；
//! - **指令只转发**：思流照亮指令 JSON 由内核产出，网关原样排队投递。
//!
//! 顾问审核建议落点（2026-09-07 并入）：
//! 1. `energy.last` 纳入快照（宿主侧维护台账尾值 {step, stored, budget}，不新增内核 FFI）；
//! 2. 快照顶层预留 `extensions` 空对象槽（未来扩展位）；
//! 3. 单写者语义显式声明：`/v1/health` 返回 `"writer":"single"`，且本文档注释固化。

pub mod http;

use std::ffi::CStr;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, RwLock};
use std::thread;

/// 快照 schema 版本（兼容规则：只增不改、旧版本可读）。
pub const SNAPSHOT_SCHEMA: u32 = 1;
/// 低能量预警阈值（内核 LOW_ENERGY_THRESHOLD=0.206 同源语义；网关侧仅作边沿投影）。
pub const LOW_ENERGY_THRESHOLD: f32 = 0.206;
/// 物态名（索引 = code）。
pub const STATE_NAMES: [&str; 4] = ["Energy", "Gas", "Liquid", "Solid"];

/// 内核读数的只读投影（SSE 跨线程转发的唯一缓存；源=npb 直读，无业务派生）。
#[derive(Clone, Debug, PartialEq)]
pub struct Projection {
    /// 宿主侧累计 push 次数（= 内核推进 tick 数；与 npb KERNEL 内 pulse 同进度）。
    pub t: u64,
    pub flow: u32,
    pub budget: u32,
    pub stored: f32,
    pub ratio: f32,
    pub absorbed: f32,
    pub spent: f32,
    pub self_intensity: f32,
    pub anchor_distance: f32,
    pub anchor_band: u32,
    pub mirror_dominant: f32,
    pub mirror_in_phase: u32,
    pub gate_pass: u64,
    pub gate_recycled: u64,
    pub gate_rejected: u64,
    pub entropy: f32,
    /// 储备低于低能量阈值的布尔投影（边沿事件依据）。
    pub low_energy: bool,
    /// A·观察台账尾值（顾问建议 1：energy.last 入快照；宿主维护，零内核改动）。
    pub energy_last: (u64, f32, u32),
    /// 思流照亮待投递指令（内核产出 JSON，网关原样转发；消费式取出）。
    pub instructions: Vec<String>,
}

impl Projection {
    fn empty() -> Self {
        Self {
            t: 0,
            flow: 0,
            budget: 0,
            stored: 0.0,
            ratio: 0.0,
            absorbed: 0.0,
            spent: 0.0,
            self_intensity: 0.0,
            anchor_distance: 0.0,
            anchor_band: 0,
            mirror_dominant: 0.0,
            mirror_in_phase: 0,
            gate_pass: 0,
            gate_recycled: 0,
            gate_rejected: 0,
            entropy: 1.0,
            low_energy: false,
            energy_last: (0, 0.0, 0),
            instructions: Vec::new(),
        }
    }

    /// 序列化为快照 JSON（schema v1）。数字 6 位小数（与 persist 风格一致）。
    pub fn to_json(&self) -> String {
        let f = |x: f32| format!("{:.6}", x);
        let el = self.energy_last;
        format!(
            concat!(
                "{{\"schema\":{sc},\"t\":{t},",
                "\"state\":{{\"flow\":{flow},\"budget\":{budget}}},",
                "\"energy\":{{\"stored\":{stored},\"ratio\":{ratio},\"absorbed\":{ab},",
                "\"spent\":{sp},\"last\":{{\"step\":{ls},\"stored\":{lv},\"budget\":{lb}}}}},",
                "\"self\":{self_i},",
                "\"anchor\":{{\"distance\":{ad},\"band\":{band}}},",
                "\"mirror\":{{\"dominant\":{md},\"in_phase\":{mi}}},",
                "\"gate\":{{\"pass\":{gp},\"recycled\":{gr},\"rejected\":{gj}}},",
                "\"entropy\":{ent},",
                "\"low_energy\":{le},",
                "\"extensions\":{{}}}}"
            ),
            sc = SNAPSHOT_SCHEMA,
            t = self.t,
            flow = self.flow,
            budget = self.budget,
            stored = f(self.stored),
            ratio = f(self.ratio),
            ab = f(self.absorbed),
            sp = f(self.spent),
            ls = el.0,
            lv = f(el.1),
            lb = el.2,
            self_i = f(self.self_intensity),
            ad = f(self.anchor_distance),
            band = self.anchor_band,
            md = f(self.mirror_dominant),
            mi = self.mirror_in_phase,
            gp = self.gate_pass,
            gr = self.gate_recycled,
            gj = self.gate_rejected,
            ent = f(self.entropy),
            le = if self.low_energy { "true" } else { "false" },
        )
    }
}

/// 快照边沿事件（字段翻转才推送；低能量为布尔边沿）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateChange {
    pub field: &'static str,
    pub from: u32,
    pub to: u32,
}

/// 自我感量化档（0..=10，降噪用：档变才推）。
fn self_band(v: f32) -> u32 {
    ((v.clamp(0.0, 1.0)) * 10.0) as u32
}

/// 对比前后投影，产出应推送的边沿事件集（顺序稳定：flow/budget/band/self/low_energy）。
pub fn edges(prev: &Projection, now: &Projection) -> Vec<StateChange> {
    let mut out = Vec::new();
    if prev.flow != now.flow {
        out.push(StateChange { field: "flow", from: prev.flow, to: now.flow });
    }
    if prev.budget != now.budget {
        out.push(StateChange { field: "budget", from: prev.budget, to: now.budget });
    }
    if prev.anchor_band != now.anchor_band {
        out.push(StateChange {
            field: "anchor_band",
            from: prev.anchor_band,
            to: now.anchor_band,
        });
    }
    if self_band(prev.self_intensity) != self_band(now.self_intensity) {
        out.push(StateChange {
            field: "self_band",
            from: self_band(prev.self_intensity),
            to: self_band(now.self_intensity),
        });
    }
    if prev.low_energy != now.low_energy {
        out.push(StateChange {
            field: "low_energy",
            from: u32::from(prev.low_energy),
            to: u32::from(now.low_energy),
        });
    }
    out
}

/// 内核访问请求（全部经 mpsc 串行到 kern 线程 —— 单一写者语义）。
enum KernMsg {
    Push { seed: f32, ack: std::sync::mpsc::SyncSender<bool> },
    PersistSnapshot { ack: std::sync::mpsc::SyncSender<String> },
    PersistRestore { bytes: Vec<u8>, ack: std::sync::mpsc::SyncSender<bool> },
}

/// L3 网关（宿主嵌入）：`push` 注入扰动、`snapshot_json` 读投影、`instructions` 供 SSE 消费。
pub struct Gateway {
    tx: Sender<KernMsg>,
    proj: Arc<RwLock<Projection>>,
}

impl Gateway {
    /// 启动内核工作线程（每进程一个网关实例；npb 内核 TLS 绑定该线程）。
    pub fn spawn() -> Self {
        let proj = Arc::new(RwLock::new(Projection::empty()));
        let (tx, rx) = channel::<KernMsg>();
        let p2 = Arc::clone(&proj);
        thread::spawn(move || kern_loop(rx, p2));
        Self { tx, proj }
    }

    /// 注入扰动（外部 push 驱动内核；负值被拒，返回 false = gate_rejected）。
    pub fn push(&self, seed: f32) -> bool {
        let (ack_tx, ack_rx) = std::sync::mpsc::sync_channel::<bool>(1);
        if self
            .tx
            .send(KernMsg::Push { seed, ack: ack_tx })
            .is_err()
        {
            return false;
        }
        ack_rx.recv().unwrap_or(false)
    }

    /// 导出内核持久化快照 JSON（kern 线程直通 npb persist_snapshot_json/free）。
    pub fn persist_snapshot(&self) -> String {
        let (ack_tx, ack_rx) = std::sync::mpsc::sync_channel::<String>(1);
        if self.tx.send(KernMsg::PersistSnapshot { ack: ack_tx }).is_err() {
            return "{}".to_string();
        }
        ack_rx.recv().unwrap_or_else(|_| "{}".to_string())
    }

    /// 恢复内核持久化快照（kern 线程写 TLS 槽 + persist_apply；成功 → true）。
    pub fn persist_restore(&self, body: &str) -> bool {
        let (ack_tx, ack_rx) = std::sync::mpsc::sync_channel::<bool>(1);
        if self
            .tx
            .send(KernMsg::PersistRestore { bytes: body.as_bytes().to_vec(), ack: ack_tx })
            .is_err()
        {
            return false;
        }
        ack_rx.recv().unwrap_or(false)
    }

    /// 只读投影副本（HTTP/SSE 读侧）。
    pub fn projection(&self) -> Projection {
        self.proj.read().expect("projection lock").clone()
    }

    /// 当前快照 JSON（GET /v1/state）。
    pub fn snapshot_json(&self) -> String {
        self.projection().to_json()
    }

    /// SSE 取走当前批次指令（消费式；多订阅者会竞争，一期推荐单订阅者，见设计 §9）。
    pub fn take_instructions(&self) -> Vec<String> {
        let mut w = self.proj.write().expect("projection write");
        std::mem::take(&mut w.instructions)
    }
}

/// 内核工作线程：唯一允许触碰 npb KERNEL 的地方。每次 push 后全量刷新投影 + drain 指令。
fn kern_loop(rx: Receiver<KernMsg>, proj: Arc<RwLock<Projection>>) {
    let mut t: u64 = 0;
    while let Ok(msg) = rx.recv() {
        match msg {
            KernMsg::Push { seed, ack } => {
                if seed < 0.0 {
                    // 不杀生：负扰动在入口即拒（与内核 npb 入口语义一致），不推进演化
                    let _ = ack.send(false);
                    continue;
                }
                npb::push_seed(seed);
                t += 1;
                let p = refresh_projection(t);
                // 先落投影、再回 ack：push() 返回后调用方读到的一定是新快照
                *proj.write().expect("projection write") = p;
                let _ = ack.send(true);
            }
            KernMsg::PersistSnapshot { ack } => {
                let _ = ack.send(snapshot_persist_json());
            }
            KernMsg::PersistRestore { bytes, ack } => {
                let ok = restore_persist_bytes(&bytes);
                if ok {
                    // 恢复改变储备/物态/自我感 → 刷新投影（t 语义=push 次数，不回溯）
                    let p = refresh_projection(t);
                    *proj.write().expect("projection write") = p;
                }
                let _ = ack.send(ok);
            }
        }
    }
}

/// kern 线程内直通 npb 持久化取快照（读 CString 后 free）。
fn snapshot_persist_json() -> String {
    // SAFETY: 指针来自 persist_snapshot_json，只读一次后释放一次。
    unsafe {
        let ptr = npb::persist_snapshot_json();
        if ptr.is_null() {
            return "{}".to_string();
        }
        let s = CStr::from_ptr(ptr).to_string_lossy().into_owned();
        npb::persist_snapshot_free(ptr);
        if s.is_empty() { "{}".to_string() } else { s }
    }
}

/// kern 线程内恢复：字节写入 TLS 加载槽（当前线程 = kern 线程）后 apply。
fn restore_persist_bytes(bytes: &[u8]) -> bool {
    if bytes.is_empty() || bytes.len() > 8192 {
        return false;
    }
    let cap = npb::persist_load_buf_cap() as usize;
    if bytes.len() > cap {
        return false;
    }
    let ptr = npb::persist_load_buf_ptr();
    if ptr.is_null() {
        return false;
    }
    // SAFETY: 槽容量 ≥ bytes.len()（上面已校验），写后 apply 在同一线程。
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
    }
    npb::persist_apply(bytes.len() as u32) == 1
}

/// 全量直读 npb（kern 线程内调用）→ 投影。
fn refresh_projection(t: u64) -> Projection {
    let stored = npb::get_energy_stored();
    let budget = npb::get_state_budget();
    let mut instructions = Vec::new();
    while npb::get_instruction_count() > 0 {
        let ptr = npb::pop_instruction_json();
        if ptr.is_null() {
            break;
        }
        // SAFETY: ptr 来自 pop_instruction_json（内核 CString），此处读取后释放一次。
        let s = unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned();
        npb::free_instruction_json(ptr);
        if !s.is_empty() {
            instructions.push(s);
        }
    }
    Projection {
        t,
        flow: npb::get_state(),
        budget,
        stored,
        ratio: npb::get_energy_ratio(),
        absorbed: npb::get_energy_absorbed(),
        spent: npb::get_energy_spent(),
        self_intensity: npb::get_self_intensity(),
        anchor_distance: npb::get_anchor_distance(),
        anchor_band: npb::get_anchor_band(),
        mirror_dominant: npb::get_mirror_dominant(),
        mirror_in_phase: npb::get_mirror_in_phase(),
        gate_pass: npb::get_gate_pass_count() as u64,
        gate_recycled: npb::get_gate_recycle_count() as u64,
        gate_rejected: npb::get_gate_reject_count() as u64,
        entropy: npb::get_entropy(),
        low_energy: stored < LOW_ENERGY_THRESHOLD,
        energy_last: (t, stored, budget),
        instructions,
    }
}

/// /v1/health 载荷：digest（跨平台一致性可观测）+ 单写者语义声明（顾问建议 3）。
pub fn health_json() -> String {
    let d = npb::mk_self_test();
    format!("{{\"ok\":true,\"digest\":{d},\"writer\":\"single\",\"schema\":{sc}}}", sc = SNAPSHOT_SCHEMA)
}

/// 极简 JSON 数字提取：从 `{"seed": <num>}` 形请求体中取 seed（协议自控，无嵌套）。
/// 返回 None 表示未找到合法数字。
pub fn parse_seed_body(body: &str) -> Option<f32> {
    let idx = body.find("\"seed\"")?;
    let rest = &body[idx + 6..];
    let start = rest.find(':')? + 1;
    let num: String = rest[start..]
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-' || *c == '+')
        .collect();
    if num.is_empty() {
        return None;
    }
    num.parse::<f32>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_json_has_schema_state_and_extensions_slot() {
        let p = Projection::empty();
        let j = p.to_json();
        assert!(j.starts_with('{') && j.ends_with('}'), "valid json: {j}");
        assert!(j.contains("\"schema\":1"));
        assert!(j.contains("\"state\":{\"flow\":0,\"budget\":0}"));
        assert!(j.contains("\"energy\":{\"stored\":0.000000"));
        // 顾问建议 2：extensions 预留槽
        assert!(j.contains("\"extensions\":{}"));
        // 顾问建议 1：energy.last 在快照内
        assert!(j.contains("\"last\":{\"step\":0,\"stored\":0.000000,\"budget\":0}"));
    }

    #[test]
    fn edges_only_on_band_change() {
        let a = Projection::empty();
        let mut b = Projection::empty();
        assert_eq!(edges(&a, &b), vec![]);
        b.budget = 2;
        b.low_energy = true;
        b.self_intensity = 0.31; // self_band 0→3
        let e = edges(&a, &b);
        assert_eq!(
            e,
            vec![
                StateChange { field: "budget", from: 0, to: 2 },
                StateChange { field: "self_band", from: 0, to: 3 },
                StateChange { field: "low_energy", from: 0, to: 1 },
            ]
        );
    }

    #[test]
    fn self_band_no_events_below_0_1_delta() {
        let a = Projection::empty();
        let mut b = Projection::empty();
        b.self_intensity = 0.09; // 仍 band 0
        assert_eq!(edges(&a, &b).iter().filter(|e| e.field == "self_band").count(), 0);
    }

    #[test]
    fn parse_seed_accepts_positive_negative_and_rejects_garbage() {
        assert_eq!(parse_seed_body(r#"{"seed": 0.5}"#), Some(0.5));
        assert_eq!(parse_seed_body(r#"{"seed":-0.25}"#), Some(-0.25));
        assert_eq!(parse_seed_body(r#"{"tag":null}"#), None);
    }

    #[test]
    fn health_declares_single_writer() {
        let h = health_json();
        assert!(h.contains("\"writer\":\"single\""));
        assert!(h.contains("\"ok\":true"));
    }

    #[test]
    fn gateway_push_drives_projection_tick() {
        let g = Gateway::spawn();
        assert!(g.push(0.5));
        let p = g.projection();
        assert_eq!(p.t, 1);
        assert!(p.energy_last.0 == 1, "energy.last step = t");
        assert!(g.push(-1.0) == false, "负值被拒");
        assert_eq!(g.projection().t, 1, "被拒不推进 tick");
    }

    #[test]
    fn snapshot_after_pushes_has_positive_stored() {
        let g = Gateway::spawn();
        for _ in 0..10 {
            g.push(0.7);
        }
        let p = g.projection();
        assert!(p.t == 10);
        assert!(p.stored > 0.0, "吸收后储备应 >0: {}", p.stored);
        assert!(p.gate_pass + p.gate_recycled + p.gate_rejected == 10, "每次 push 一次闸门判定");
        let j = p.to_json();
        assert!(j.contains("\"t\":10"));
    }
}
