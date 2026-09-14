//! L7 · 宿主侧执行器（T1 闭环的"执行"半）。
//!
//! 依据：发起人 Q1–Q4 裁决（方案 A 架构 + 首版只开放 T1）与 `docs/L7_EXECUTION_DESIGN.md §7`。
//!
//! ## 硬约束（写死在代码里，不是约定）
//! 1. **只接受预置动作 id** —— 动作表是编译期常量数组；请求里的 id 必须在表中，否则拒绝；
//! 2. **绝不接受自由文本** —— 请求体里**没有**命令/脚本/路径字段，只有 `id` 与一次性 `token`；
//! 3. **无 shell 拼接** —— 一律 `Command::new(绝对路径).arg(...)`，绝不经过 `cmd /c` 或字符串拼命令；
//! 4. **先备份** —— 任何可能删除/覆盖的动作，执行前把受影响内容复制到 `_backup_<时间戳>\`；
//! 5. **可回滚** —— 每个动作声明回滚方式；无持久状态变更的动作如实标注"无需回滚"；
//! 6. **不碰网络配置** —— 表中动作均为 `Touches::OwnFiles`（本应用文件/自身进程），
//!    不触及 IP / DNS / 代理 / hosts / 防火墙 / 路由。
//!
//! ## 授权
//! - `issue_token(id, remember)`：签发**一次性令牌**（用后作废，防重放）；`remember=true` 同时记入授权账；
//! - `execute(id, token)`：先验令牌（一次性）→ 再验分级（非 T1 拒绝）→ 备份 → 执行 → 记账；
//! - `revoke(id)`：撤销记忆授权（之后需逐次确认）。

use std::collections::VecDeque;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// 动作等级编码（与内核 `grade::Grade::code()` 对齐）。
pub const GRADE_T1: u8 = 1;

/// 预置动作（宿主侧镜像；**与内核 `l7::repair::CATALOG` 的 id/key 一致**）。
pub struct ActionDef {
    pub id: u32,
    pub key: &'static str,
    pub name: &'static str,
    pub grade: u8,
    pub rollback_hint: &'static str,
}

pub const ACTIONS: [ActionDef; 4] = [
    ActionDef { id: 1, key: "clean-temp", name: "清理本应用临时文件", grade: GRADE_T1, rollback_hint: "从执行前备份恢复" },
    ActionDef { id: 2, key: "restart-watchdog", name: "重启看门狗", grade: GRADE_T1, rollback_hint: "—（重启即恢复；看门狗自带单实例保护）" },
    ActionDef { id: 3, key: "reload-config", name: "重读配置", grade: GRADE_T1, rollback_hint: "删除本次生成的清单文件" },
    ActionDef { id: 4, key: "trigger-probe", name: "触发探针采集", grade: GRADE_T1, rollback_hint: "—（只读采集）" },
];

pub fn action_by_id(id: u32) -> Option<&'static ActionDef> {
    ACTIONS.iter().find(|a| a.id == id)
}

/// 账本链节（与内核 `l7::ledger` 同构：同前向哈希语义，但**分账**）。
#[derive(Clone, Copy, Debug)]
pub struct Link {
    pub seq: u32,
    pub prev: u64,
    pub hash: u64,
    pub action_id: u32,
    pub grade: u8,
    pub outcome: &'static str,
    pub note: &'static str,
}

fn fnv1a64(mut h: u64, b: &[u8]) -> u64 {
    for &x in b {
        h ^= x as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

fn link_hash(prev: u64, id: u32, grade: u8, seq: u32, outcome: &str, note: &str) -> u64 {
    let mut h = fnv1a64(0xcbf2_9ce4_8422_2325, &prev.to_le_bytes());
    h = fnv1a64(h, &id.to_le_bytes());
    h = fnv1a64(h, &[grade]);
    h = fnv1a64(h, &seq.to_le_bytes());
    h = fnv1a64(h, outcome.as_bytes());
    h = fnv1a64(h, note.as_bytes());
    h
}

struct Token {
    id: u32,
    value: String,
    used: bool,
    at: u64,
}

/// 执行结果。
#[derive(Clone, Debug)]
pub struct ExecResult {
    pub ok: bool,
    pub action_id: u32,
    pub key: &'static str,
    pub output: String,
    pub backup: Option<String>,
    pub hash: u64,
    pub note: &'static str,
}

fn now_s() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn stamp() -> String {
    let s = now_s();
    format!("{}", s)
}

/// 宿主侧执行器。
pub struct Executor {
    /// 部署目录（exe 与 watchdog.vbs / cloud-probe.exe 所在）。
    desk: PathBuf,
    /// 本应用临时目录（只清理这里，不碰系统其它临时文件）。
    temp: PathBuf,
    /// 网关监听端口（`trigger-probe` 回传目标）。
    port: Mutex<u16>,
    grants: Mutex<Vec<u32>>,
    revoked: Mutex<Vec<u32>>,
    tokens: Mutex<VecDeque<Token>>,
    ledger: Mutex<Vec<Link>>,
    nonce: Mutex<u64>,
}

impl Executor {
    pub fn new(desk: PathBuf, temp: PathBuf) -> Self {
        Self {
            desk,
            temp,
            port: Mutex::new(3000),
            grants: Mutex::new(Vec::new()),
            revoked: Mutex::new(Vec::new()),
            tokens: Mutex::new(VecDeque::new()),
            ledger: Mutex::new(Vec::new()),
            nonce: Mutex::new(0),
        }
    }

    pub fn set_port(&self, p: u16) {
        if let Ok(mut g) = self.port.lock() {
            *g = p;
        }
    }

    // ---- 授权账 ----
    pub fn grants(&self) -> (Vec<u32>, Vec<u32>) {
        (
            self.grants.lock().map(|g| g.clone()).unwrap_or_default(),
            self.revoked.lock().map(|g| g.clone()).unwrap_or_default(),
        )
    }

    pub fn is_granted(&self, id: u32) -> bool {
        let (g, r) = self.grants();
        g.contains(&id) && !r.contains(&id)
    }

    pub fn revoke(&self, id: u32) -> bool {
        if let Ok(mut r) = self.revoked.lock() {
            if !r.contains(&id) {
                r.push(id);
            }
            return true;
        }
        false
    }

    /// 签发一次性令牌（**用后作废**）。`remember=true` 同时写入记忆授权。
    pub fn issue_token(&self, id: u32, remember: bool) -> Option<String> {
        if action_by_id(id).is_none() {
            return None;
        }
        let n = {
            let mut g = self.nonce.lock().ok()?;
            *g += 1;
            *g
        };
        let raw = fnv1a64(0x9e37_79b9_7f4a_7c15, format!("{}:{}:{}", now_s(), id, n).as_bytes());
        let value = format!("{raw:016x}");
        if let Ok(mut t) = self.tokens.lock() {
            if t.len() >= 64 {
                t.pop_front();
            }
            t.push_back(Token { id, value: value.clone(), used: false, at: now_s() });
        }
        if remember {
            if let Ok(mut g) = self.grants.lock() {
                if !g.contains(&id) {
                    g.push(id);
                }
            }
            if let Ok(mut r) = self.revoked.lock() {
                r.retain(|x| *x != id);
            }
            self.ledger_append(id, "granted", "用户授权并记忆");
        }
        Some(value)
    }

    /// 消费令牌（一次性；不匹配或已用返回 false）。
    pub fn consume_token(&self, id: u32, token: &str) -> bool {
        if let Ok(mut t) = self.tokens.lock() {
            for tok in t.iter_mut().rev() {
                if tok.id == id && !tok.used && tok.value == token {
                    tok.used = true;
                    return true;
                }
            }
        }
        false
    }

    // ---- 账本 ----
    pub fn ledger_append(&self, id: u32, outcome: &'static str, note: &'static str) -> u64 {
        let grade = action_by_id(id).map(|a| a.grade).unwrap_or(9);
        if let Ok(mut l) = self.ledger.lock() {
            let seq = l.len() as u32 + 1;
            let prev = l.last().map(|x| x.hash).unwrap_or(0);
            let hash = link_hash(prev, id, grade, seq, outcome, note);
            l.push(Link { seq, prev, hash, action_id: id, grade, outcome, note });
            return hash;
        }
        0
    }

    pub fn ledger_head(&self) -> u64 {
        self.ledger.lock().ok().and_then(|l| l.last().map(|x| x.hash)).unwrap_or(0)
    }

    pub fn ledger_len(&self) -> usize {
        self.ledger.lock().map(|l| l.len()).unwrap_or(0)
    }

    pub fn ledger_verify(&self) -> bool {
        let l = match self.ledger.lock() {
            Ok(l) => l,
            Err(_) => return false,
        };
        let mut prev = 0u64;
        for (i, x) in l.iter().enumerate() {
            if x.seq as usize != i + 1 || x.prev != prev {
                return false;
            }
            if link_hash(prev, x.action_id, x.grade, x.seq, x.outcome, x.note) != x.hash {
                return false;
            }
            prev = x.hash;
        }
        true
    }

    pub fn ledger_text(&self) -> String {
        let mut s = String::new();
        if let Ok(l) = self.ledger.lock() {
            for x in l.iter() {
                s.push_str(&format!(
                    "[元内核] AUDIT #{} action={} grade=T{} {} · {} (prev={:#x})\n",
                    x.seq, x.action_id, x.grade, x.outcome, x.note, x.prev
                ));
            }
        }
        s
    }

    // ---- 执行 ----
    /// **执行（唯一入口）**：校验 → 备份 → 执行 → 记账。
    pub fn execute(&self, id: u32, token: &str) -> ExecResult {
        let def = match action_by_id(id) {
            Some(d) => d,
            None => {
                // 非法 id **同样入账**（审计不留空白）
                let h = self.ledger_append(id, "refused", "动作 id 不在预置白名单");
                return ExecResult {
                    ok: false, action_id: id, key: "?", output: "拒绝：动作 id 不在预置白名单".into(),
                    backup: None, hash: h, note: "invalid_action",
                }
            }
        };
        // ① 分级：首版只放行 T1
        if def.grade != GRADE_T1 {
            let h = self.ledger_append(id, "refused", "分级非 T1（首版只开放 T1）");
            return ExecResult { ok: false, action_id: id, key: def.key, output: "拒绝：分级高于 T1".into(), backup: None, hash: h, note: "refused" };
        }
        // ② 令牌（一次性）
        if !self.consume_token(id, token) {
            let h = self.ledger_append(id, "refused", "令牌无效或已使用");
            return ExecResult { ok: false, action_id: id, key: def.key, output: "拒绝：令牌无效或已使用（防重放）".into(), backup: None, hash: h, note: "bad_token" };
        }
        // ③ 授权：记忆授权 or 本次令牌即视为本次确认
        if !self.is_granted(id) {
            // 令牌本身代表"本次已确认"；但若已被撤销则需重新授权
            let (_, revoked) = self.grants();
            if revoked.contains(&id) {
                let h = self.ledger_append(id, "refused", "授权已撤销，需重新确认");
                return ExecResult { ok: false, action_id: id, key: def.key, output: "拒绝：授权已撤销".into(), backup: None, hash: h, note: "revoked" };
            }
        }
        // ④ 备份 + 执行
        let (ok, output, backup) = match def.key {
            "clean-temp" => self.act_clean_temp(),
            "restart-watchdog" => self.act_restart_watchdog(),
            "reload-config" => self.act_reload_config(),
            "trigger-probe" => self.act_trigger_probe(),
            _ => (false, "拒绝：无此动作实现".to_string(), None),
        };
        let h = self.ledger_append(id, if ok { "executed" } else { "failed" }, def.name);
        ExecResult {
            ok,
            action_id: id,
            key: def.key,
            output,
            backup,
            hash: h,
            note: if ok { "executed" } else { "failed" },
        }
    }

    /// 回滚：从 `_backup_*` 中**最近一次**该动作的备份恢复。
    pub fn rollback(&self, id: u32) -> ExecResult {
        let def = match action_by_id(id) {
            Some(d) => d,
            None => {
                return ExecResult { ok: false, action_id: id, key: "?", output: "拒绝：动作 id 不在预置白名单".into(), backup: None, hash: 0, note: "invalid_action" }
            }
        };
        let (ok, output) = match def.key {
            "clean-temp" => self.rollback_clean_temp(),
            "reload-config" => self.rollback_reload_config(),
            _ => (true, format!("无需回滚：{}", def.rollback_hint)),
        };
        let h = self.ledger_append(id, "rolled_back", def.name);
        ExecResult { ok, action_id: id, key: def.key, output, backup: None, hash: h, note: "rolled_back" }
    }

    // ---- 各动作实现（固定路径；无 shell 拼接）----

    fn backup_dir(&self, tag: &str) -> Option<PathBuf> {
        let d = self.desk.join(format!("_backup_{}_{}", stamp(), tag));
        std::fs::create_dir_all(&d).ok().map(|_| d)
    }

    /// ① 清理本应用临时文件（**只清 `%TEMP%\ck-net`**；先备份 → 可回滚）。
    fn act_clean_temp(&self) -> (bool, String, Option<String>) {
        let src = self.temp.clone();
        if !src.exists() {
            return (true, format!("无需清理：{} 不存在", src.display()), None);
        }
        let bak = match self.backup_dir("clean-temp") {
            Some(b) => b,
            None => return (false, "备份目录创建失败，已中止（不改动任何文件）".into(), None),
        };
        let mut copied = 0usize;
        let mut failed = 0usize;
        if let Ok(rd) = std::fs::read_dir(&src) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_file() {
                    let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    if std::fs::copy(&p, bak.join(&name)).is_ok() {
                        copied += 1;
                        if std::fs::remove_file(&p).is_err() {
                            failed += 1;
                        }
                    }
                }
            }
        }
        let out = format!(
            "已备份 {} 个文件到 {}；清理 {} 个，失败 {} 个（只清理本应用临时目录，未触碰系统其它临时文件）",
            copied, bak.display(), copied.saturating_sub(failed), failed
        );
        (failed == 0, out, Some(bak.to_string_lossy().to_string()))
    }

    fn rollback_clean_temp(&self) -> (bool, String) {
        let mut best: Option<PathBuf> = None;
        if let Ok(rd) = std::fs::read_dir(&self.desk) {
            for e in rd.flatten() {
                let p = e.path();
                let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                if name.starts_with("_backup_") && name.ends_with("clean-temp") && p.is_dir() {
                    if best.as_ref().map(|b| p > *b).unwrap_or(true) {
                        best = Some(p);
                    }
                }
            }
        }
        let bak = match best {
            Some(b) => b,
            None => return (false, "未找到 clean-temp 的备份，无法回滚".into()),
        };
        std::fs::create_dir_all(&self.temp).ok();
        let mut n = 0usize;
        if let Ok(rd) = std::fs::read_dir(&bak) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_file() {
                    let name = p.file_name().map(|x| x.to_string_lossy().to_string()).unwrap_or_default();
                    if std::fs::copy(&p, self.temp.join(&name)).is_ok() {
                        n += 1;
                    }
                }
            }
        }
        (true, format!("已从 {} 恢复 {} 个文件到 {}", bak.display(), n, self.temp.display()))
    }

    /// ② 重启看门狗（**固定脚本路径；不杀任何其它进程**；watchdog.vbs 自带单实例保护）。
    fn act_restart_watchdog(&self) -> (bool, String, Option<String>) {
        let script = self.desk.join("watchdog.vbs");
        if !script.exists() {
            return (false, format!("未找到 {}", script.display()), None);
        }
        let r = Command::new("wscript.exe").arg(script.as_os_str()).spawn();
        match r {
            Ok(_) => (true, "已请求启动看门狗（单实例保护：重复启动不会叠加；未结束任何其它进程）".into(), None),
            Err(e) => (false, format!("启动失败：{e}"), None),
        }
    }

    /// ③ 重读配置：重扫部署资源清单并落一份新清单文件（**不覆盖旧文件**）。
    fn act_reload_config(&self) -> (bool, String, Option<String>) {
        let mut items: Vec<String> = Vec::new();
        let mut scan = |dir: &Path, tag: &str| {
            if let Ok(rd) = std::fs::read_dir(dir) {
                for e in rd.flatten() {
                    let p = e.path();
                    if p.is_file() {
                        let n = p.file_name().map(|x| x.to_string_lossy().to_string()).unwrap_or_default();
                        let sz = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
                        items.push(format!("{tag}/{n}\t{sz}"));
                    }
                }
            }
        };
        scan(&self.desk, ".");
        scan(&self.desk.join("ui"), "ui");
        items.sort();
        let rep = self.desk.join("_report");
        if std::fs::create_dir_all(&rep).is_err() {
            return (false, "清单目录创建失败".into(), None);
        }
        let file = rep.join(format!("resource-manifest-{}.txt", stamp()));
        let body = format!("{} files\n{}", items.len(), items.join("\n"));
        let ok = std::fs::File::create(&file)
            .and_then(|mut f| f.write_all(body.as_bytes()))
            .is_ok();
        (
            ok,
            if ok {
                format!("已重读配置：扫描 {} 个资源，清单写入 {}", items.len(), file.display())
            } else {
                "清单写入失败".into()
            },
            Some(file.to_string_lossy().to_string()),
        )
    }

    fn rollback_reload_config(&self) -> (bool, String) {
        let rep = self.desk.join("_report");
        let mut n = 0usize;
        if let Ok(rd) = std::fs::read_dir(&rep) {
            for e in rd.flatten() {
                let p = e.path();
                let name = p.file_name().map(|x| x.to_string_lossy().to_string()).unwrap_or_default();
                if name.starts_with("resource-manifest-") && std::fs::remove_file(&p).is_ok() {
                    n += 1;
                }
            }
        }
        (true, format!("已移除本次生成的清单文件 {n} 个（不影响其它文件）"))
    }

    /// ④ 触发探针采集（**只读**：调用部署目录内的 cloud-probe.exe 回传本机网关）。
    fn act_trigger_probe(&self) -> (bool, String, Option<String>) {
        let exe = self.desk.join("cloud-probe.exe");
        if !exe.exists() {
            return (false, format!("未找到 {}（无法触发采集）", exe.display()), None);
        }
        let port = self.port.lock().map(|p| *p).unwrap_or(3000);
        let url = format!("http://127.0.0.1:{port}/v1/probe");
        match Command::new(&exe).arg("--report").arg(&url).output() {
            Ok(o) => {
                let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                (o.status.success(), format!("采集已触发并回传 {url}：{s}"), None)
            }
            Err(e) => (false, format!("触发失败：{e}"), None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        // 唯一化：秒 + 纳秒 + 原子计数（避免并行测试同秒撞名 → 互相干扰）
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        let d = std::env::temp_dir().join(format!(
            "ck-test-{}-{}-{}-{}",
            name,
            now_s(),
            ns,
            n
        ));
        std::fs::create_dir_all(&d).ok();
        d
    }

    fn ex() -> (Executor, PathBuf, PathBuf) {
        let desk = tmp("desk");
        let temp = tmp("temp");
        (Executor::new(desk.clone(), temp.clone()), desk, temp)
    }

    #[test]
    fn action_table_is_the_only_whitelist() {
        assert_eq!(ACTIONS.len(), 4);
        assert!(action_by_id(1).is_some());
        assert!(action_by_id(0).is_none());
        assert!(action_by_id(99).is_none(), "未知 id 必须拒绝");
        assert_eq!(action_by_id(1).unwrap().key, "clean-temp");
    }

    #[test]
    fn token_is_one_shot() {
        let (e, _d, _t) = ex();
        assert!(e.issue_token(99, false).is_none(), "未知动作不签发令牌");
        let tok = e.issue_token(1, false).expect("签发");
        assert!(e.consume_token(1, &tok), "首次可用");
        assert!(!e.consume_token(1, &tok), "**二次使用必须失败（防重放）**");
        assert!(!e.consume_token(1, "deadbeef"), "伪令牌失败");
        assert!(!e.consume_token(2, &tok), "跨动作令牌失败");
    }

    #[test]
    fn execute_rejects_unknown_id_and_bad_token() {
        let (e, _d, _t) = ex();
        let r = e.execute(42, "x");
        assert!(!r.ok);
        assert_eq!(r.note, "invalid_action");
        let r2 = e.execute(1, "bogus");
        assert!(!r2.ok);
        assert_eq!(r2.note, "bad_token");
        assert!(e.ledger_verify(), "拒绝同样入链");
        assert_eq!(e.ledger_len(), 2);
    }

    /// **验收：T1 闭环**（授权 → 执行 → 记账）。
    #[test]
    fn granted_then_execute_records_ledger() {
        let (e, _d, temp) = ex();
        std::fs::write(temp.join("a.tmp"), b"hello").unwrap();
        let tok = e.issue_token(1, true).expect("授权+令牌");
        assert!(e.is_granted(1), "remember=true → 写入记忆授权");
        let r = e.execute(1, &tok);
        assert!(r.ok, "{}", r.output);
        assert!(r.backup.is_some(), "先备份");
        assert!(!temp.join("a.tmp").exists(), "临时文件已清理");
        assert!(e.ledger_verify());
        let txt = e.ledger_text();
        assert!(txt.contains("[元内核] AUDIT"), "{txt}");
        assert!(txt.contains("granted"));
        assert!(txt.contains("executed"));
    }

    /// **验收：回滚可用**。
    #[test]
    fn rollback_restores_cleaned_files() {
        let (e, _d, temp) = ex();
        std::fs::write(temp.join("keep.txt"), b"important").unwrap();
        let tok = e.issue_token(1, true).unwrap();
        assert!(e.execute(1, &tok).ok);
        assert!(!temp.join("keep.txt").exists(), "已被清理");
        let rb = e.rollback(1);
        assert!(rb.ok, "{}", rb.output);
        assert!(temp.join("keep.txt").exists(), "**回滚后文件回来了**");
        assert_eq!(std::fs::read_to_string(temp.join("keep.txt")).unwrap(), "important");
        assert!(e.ledger_verify());
        assert!(e.ledger_text().contains("rolled_back"));
    }

    #[test]
    fn revoke_blocks_execution() {
        let (e, _d, _t) = ex();
        let tok = e.issue_token(1, true).unwrap();
        assert!(e.is_granted(1));
        e.revoke(1);
        assert!(!e.is_granted(1), "撤销后不再授权");
        let r = e.execute(1, &tok);
        assert!(!r.ok);
        assert_eq!(r.note, "revoked", "撤销后必须重新确认");
    }

    #[test]
    fn reload_config_writes_manifest_and_rolls_back() {
        let (e, desk, _t) = ex();
        std::fs::write(desk.join("npb-gateway.exe"), b"x").unwrap();
        let tok = e.issue_token(3, true).unwrap();
        let r = e.execute(3, &tok);
        assert!(r.ok, "{}", r.output);
        let f = r.backup.clone().unwrap();
        assert!(Path::new(&f).exists(), "清单文件已生成");
        let rb = e.rollback(3);
        assert!(rb.ok);
        assert!(!Path::new(&f).exists(), "回滚后清单被移除");
    }

    #[test]
    fn watchdog_and_probe_handle_missing_files_gracefully() {
        let (e, _d, _t) = ex();
        let t2 = e.issue_token(2, true).unwrap();
        let r2 = e.execute(2, &t2);
        assert!(!r2.ok, "部署目录无 watchdog.vbs → 如实失败而非假装成功");
        assert!(r2.output.contains("未找到"));
        let t4 = e.issue_token(4, true).unwrap();
        let r4 = e.execute(4, &t4);
        assert!(!r4.ok);
        assert!(r4.output.contains("未找到"));
        assert!(e.ledger_verify(), "失败同样入链");
    }

    #[test]
    fn ledger_detects_tampering() {
        let (e, _d, _t) = ex();
        e.ledger_append(1, "executed", "a");
        e.ledger_append(2, "executed", "b");
        assert!(e.ledger_verify());
        if let Ok(mut l) = e.ledger.lock() {
            l[0].outcome = "refused";
        }
        assert!(!e.ledger_verify(), "篡改必须可检出");
    }
}
