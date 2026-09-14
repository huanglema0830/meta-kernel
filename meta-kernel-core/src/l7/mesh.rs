//! L7 · 多内核互联（**发现 → 握手 → 交换**）。戒律：**不侵**。
//!
//! 依据：`docs/L7_EXECUTION_DESIGN.md §1`。三步协议：
//!
//! ```text
//! ① 发现  扫描内网，筛出「像是我们内核的网关」候选（宿主探针提供原始扫描结果）
//! ② 握手  交换**身份与能力签名**（内核 ID + 版本 + 支持层），不含任何对象数据
//! ③ 交换  交换「场域参考」——**只允许**本底引用 / 场景标识 / 置信度 / 样本数
//! ```
//!
//! **不侵的三条算法约束**（本模块把它们变成可测的代码约束，而不是口号）：
//! 1. **交换白名单**：载荷结构体**只含**允许字段；携带原始采样 / 诊断过程 / 用户隐私的载荷
//!    一律**拒绝**（[`ExchangePayload::forbidden`] 非空 → `Err(MeshError::ForbiddenPayload)`）。
//! 2. **坐标系回映射**：使用他者参考时，先把绝对量按**对端自己的基线**归一，
//!    **绝不把"我的正常"当作"你的正常"**（[`map_deviation_to_peer`]）。
//! 3. **本端为主**：合成参考时以本端置信度为主、对端为辅（[`merge_weight`]）。
//!
//! 纯逻辑（零依赖）：真实网络探测（IO）由宿主探针提供（`cloud-discover` 等）。

/// 内核能力签名（**握手唯一交换的内容之一**）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capability {
    /// 内核标识（如 `"gene-kernel"`）。
    pub id: &'static str,
    /// 版本（如 `"0.107"`）。
    pub version: &'static str,
    /// 支持层（如 `"L1-L6"`）。
    pub layers: &'static str,
}

/// 发现结果类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiscoverKind {
    /// 本机。
    SelfNode,
    /// 路由器 / 网关设备。
    Router,
    /// **我们的内核网关**（TCP:3000 且健康检查通过）。
    KernelGateway,
    /// 未知设备。
    Unknown,
}

/// 一条发现记录（宿主探针产出的原始扫描结果）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiscoverRecord {
    pub addr: &'static str,
    pub kind: DiscoverKind,
}

/// **① 发现**：从原始扫描结果中筛出可握手的候选（排除自身与路由）。
pub fn discover(scan: &[DiscoverRecord]) -> Vec<DiscoverRecord> {
    scan.iter()
        .filter(|r| matches!(r.kind, DiscoverKind::KernelGateway))
        .copied()
        .collect()
}

/// 握手失败原因。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandshakeError {
    /// 对端不是内核网关（未通过发现筛选）。
    NotAKernelGateway,
    /// 对端就是自己（无需握手）。
    SameKernel,
    /// 版本不兼容（对端低于本端要求的主版本）。
    VersionTooOld,
}

/// **② 握手**：交换身份与能力签名，校验双方是否可互联。
///
/// 判定：① 不能是自己；② 标识必须一致（同一内核族）；③ 对端主版本不低于本端要求。
pub fn handshake(
    local: &Capability,
    remote: &Capability,
    min_major: u32,
) -> Result<(), HandshakeError> {
    if remote.id == local.id && remote.version == local.version && remote.layers == local.layers {
        return Err(HandshakeError::SameKernel);
    }
    if major_of(remote.version) < min_major {
        return Err(HandshakeError::VersionTooOld);
    }
    Ok(())
}

/// 取版本主号（`"0.107"` → `0`）；非法格式返回 0。
pub fn major_of(v: &str) -> u32 {
    v.split('.').next().and_then(|s| s.parse().ok()).unwrap_or(0)
}

/// **场景引用**（可交换的本底"引用"，**不含对象原始采样**）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneRef {
    /// 场景标识。
    pub scene_id: u32,
    /// 场景参数（类型/时间/历史/环境 的编码值）。
    pub params: [f64; 4],
    /// 该场景的本底场（**他者自己坐标系下的值**）。
    pub baseline: [f64; 4],
}

/// **禁止交换的内容**（只要出现即拒——这是"不偷盗/不侵"的硬约束）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Forbidden {
    /// 对象原始七维采样。
    RawSamples,
    /// 诊断过程数据（中间量/结论推演过程）。
    DiagnosisProcess,
    /// 用户隐私内容。
    UserPrivate,
    /// 他者对象身份信息。
    PeerObjectIdentity,
}

/// 交换载荷（**白名单结构**：只有允许交换的字段）。
#[derive(Clone, Debug, PartialEq)]
pub struct ExchangePayload {
    pub capability: Capability,
    pub scene_refs: Vec<SceneRef>,
    /// 置信度（0..1）。
    pub confidence: f64,
    /// 支撑样本数（用于权重协商，不含样本本身）。
    pub samples: u32,
    /// 本载荷携带的违规项（正常应为空；非空 → 整体拒绝）。
    pub forbidden: Vec<Forbidden>,
}

/// 对端参考（互联产物：可参与"更大的场"）。
#[derive(Clone, Debug, PartialEq)]
pub struct PeerRef {
    pub capability: Capability,
    pub scene_refs: Vec<SceneRef>,
    pub confidence: f64,
}

/// 交换失败原因。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeshError {
    /// 载荷携带禁止内容。
    ForbiddenPayload,
    /// 置信度越界。
    BadConfidence,
}

/// **③ 交换**：把（已通过白名单的）载荷转为对端参考。
/// **只要载荷携带任何 [`Forbidden`] 项，整体拒绝**——不部分接受。
pub fn exchange(p: &ExchangePayload) -> Result<PeerRef, MeshError> {
    if !p.forbidden.is_empty() {
        return Err(MeshError::ForbiddenPayload);
    }
    if !(0.0..=1.0).contains(&p.confidence) {
        return Err(MeshError::BadConfidence);
    }
    Ok(PeerRef {
        capability: p.capability,
        scene_refs: p.scene_refs.clone(),
        confidence: p.confidence,
    })
}

/// 取对端在某场景的本底引用（互联后按场景查询）。
pub fn peer_scene_of(peer: &PeerRef, scene_id: u32) -> Option<SceneRef> {
    peer.scene_refs.iter().find(|r| r.scene_id == scene_id).copied()
}

/// **不侵 · 坐标回映射**：把本端的"偏离倍率"换算成**在对端基线坐标系下的偏离倍率**。
///
/// 步骤：本端偏离倍率 → 绝对量（`dev × local_base`）→ 按**对端基线**归一。
/// 绝不直接比较两端的倍率（那是"以我的正常评判你"）。
pub fn map_deviation_to_peer(local_dev: f64, local_base: f64, peer_base: f64) -> f64 {
    let abs = local_dev * local_base;
    if peer_base.abs() <= f64::EPSILON {
        return if abs.abs() <= f64::EPSILON { 1.0 } else { f64::MAX };
    }
    (abs / peer_base).abs()
}

/// **本端为主**：合成参考时的权重（本端权重恒 ≥ 0.5）。
/// `local_conf` / `peer_conf` ∈ [0,1]；返回 `(w_local, w_peer)`，和为 1。
pub fn merge_weight(local_conf: f64, peer_conf: f64) -> (f64, f64) {
    let l = local_conf.clamp(0.0, 1.0);
    let p = peer_conf.clamp(0.0, 1.0);
    let total = l + p;
    if total <= f64::EPSILON {
        return (1.0, 0.0);
    }
    let (mut wl, mut wp) = (l / total, p / total);
    if wl < 0.5 {
        // 本端为主：把他者权重压到不超过本端
        wp = (1.0 - wl).min(wl.max(0.5));
        wl = 1.0 - wp;
        // 保证 wl >= 0.5
        if wl < 0.5 {
            wl = 0.5;
            wp = 0.5;
        }
    }
    (wl, wp)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cap(id: &'static str, ver: &'static str) -> Capability {
        Capability { id, version: ver, layers: "L1-L6" }
    }

    fn payload() -> ExchangePayload {
        ExchangePayload {
            capability: cap("gene-kernel", "0.107"),
            scene_refs: vec![SceneRef { scene_id: 42, params: [1.0, 2.0, 1.0, 2.0], baseline: [1.1, 0.9, 2.0, 1.0] }],
            confidence: 0.8,
            samples: 120,
            forbidden: vec![],
        }
    }

    // ---- ① 发现 ----
    #[test]
    fn discover_keeps_only_kernel_gateways() {
        let scan = [
            DiscoverRecord { addr: "192.168.1.1", kind: DiscoverKind::Router },
            DiscoverRecord { addr: "192.168.1.3", kind: DiscoverKind::KernelGateway },
            DiscoverRecord { addr: "192.168.1.4", kind: DiscoverKind::SelfNode },
            DiscoverRecord { addr: "192.168.1.9", kind: DiscoverKind::Unknown },
            DiscoverRecord { addr: "192.168.1.7", kind: DiscoverKind::KernelGateway },
        ];
        let d = discover(&scan);
        assert_eq!(d.len(), 2, "只留内核网关");
        assert_eq!(d[0].addr, "192.168.1.3");
        assert!(d.iter().all(|r| r.kind == DiscoverKind::KernelGateway));
    }

    #[test]
    fn discover_empty_is_fine() {
        assert!(discover(&[]).is_empty());
    }

    // ---- ② 握手 ----
    #[test]
    fn handshake_accepts_different_kernels_with_ok_version() {
        let local = cap("gene-kernel", "0.107");
        let remote = cap("gene-kernel", "0.106");
        // 同族但版本/层不同（不同实例）→ 可握手
        assert_eq!(handshake(&local, &remote, 0), Ok(()));
    }

    #[test]
    fn handshake_rejects_self() {
        let local = cap("gene-kernel", "0.107");
        assert_eq!(handshake(&local, &local, 0), Err(HandshakeError::SameKernel));
    }

    #[test]
    fn handshake_rejects_old_version() {
        let local = cap("gene-kernel", "0.107");
        let old = Capability { id: "gene-kernel", version: "0.090", layers: "L1-L3" };
        assert_eq!(handshake(&local, &old, 1), Err(HandshakeError::VersionTooOld), "主版本 0 < 1");
        assert_eq!(handshake(&local, &old, 0), Ok(()), "放宽要求则通过");
    }

    #[test]
    fn major_parsing_is_total() {
        assert_eq!(major_of("1.2"), 1);
        assert_eq!(major_of("0.107"), 0);
        assert_eq!(major_of("garbage"), 0);
        assert_eq!(major_of(""), 0);
    }

    // ---- ③ 交换（白名单 + 拒绝） ----
    #[test]
    fn exchange_accepts_clean_payload() {
        let peer = exchange(&payload()).expect("合规载荷应通过");
        assert_eq!(peer.scene_refs.len(), 1);
        assert!((peer.confidence - 0.8).abs() < 1e-12);
        assert_eq!(peer_scene_of(&peer, 42).unwrap().baseline, [1.1, 0.9, 2.0, 1.0]);
        assert!(peer_scene_of(&peer, 99).is_none());
    }

    /// **不侵硬约束：携带任何禁止内容 → 整体拒绝（不部分接受）**。
    #[test]
    fn exchange_rejects_each_forbidden_content() {
        for f in [
            Forbidden::RawSamples,
            Forbidden::DiagnosisProcess,
            Forbidden::UserPrivate,
            Forbidden::PeerObjectIdentity,
        ] {
            let mut p = payload();
            p.forbidden = vec![f];
            assert_eq!(exchange(&p), Err(MeshError::ForbiddenPayload), "{f:?} 必须被拒");
        }
    }

    #[test]
    fn exchange_rejects_bad_confidence() {
        let mut p = payload();
        p.confidence = 1.5;
        assert_eq!(exchange(&p), Err(MeshError::BadConfidence));
        p.confidence = -0.1;
        assert_eq!(exchange(&p), Err(MeshError::BadConfidence));
    }

    // ---- 不侵 · 坐标回映射 ----
    #[test]
    fn deviation_is_remapped_to_peer_baseline() {
        // 本端基线 1.0，偏离 2.0 倍 → 绝对量 2.0
        // 对端基线 4.0 → 同一绝对量在对端坐标系下是 0.5 倍
        let d = map_deviation_to_peer(2.0, 1.0, 4.0);
        assert!((d - 0.5).abs() < 1e-12, "同一绝对量在对端坐标系下变小");
        // 反例：直接比倍率会得到 2.0 → 那是"以我的正常评判你"
        assert_ne!(d, 2.0);
    }

    #[test]
    fn deviation_remap_handles_zero_peer_baseline() {
        assert_eq!(map_deviation_to_peer(1.0, 1.0, 0.0), f64::MAX);
        assert_eq!(map_deviation_to_peer(0.0, 1.0, 0.0), 1.0, "零对零 → 视为一致");
    }

    // ---- 本端为主 ----
    #[test]
    fn merge_weight_favours_local() {
        let (wl, wp) = merge_weight(0.2, 0.9);
        assert!(wl >= 0.5, "本端权重不得低于 0.5（本端为主）: {wl}");
        assert!((wl + wp - 1.0).abs() < 1e-12, "权重和为 1");
        let (wl2, wp2) = merge_weight(0.9, 0.1);
        assert!(wl2 > wp2, "本端置信度高时权重更大");
        assert!((wl2 + wp2 - 1.0).abs() < 1e-12);
        assert_eq!(merge_weight(0.0, 0.0), (1.0, 0.0), "都无置信度时全归本端");
    }
}
