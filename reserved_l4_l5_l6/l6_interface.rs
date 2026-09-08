// =====================================================================
// 【L6·对齐层】愿景预留接口（NOT COMPILED — 见本目录 README.md）
// 依据：docs/LAYER_ARCHITECTURE（L6 对齐层）、docs/SILA_IMPLEMENTATION（对齐机制）
// 功能：让 L5 外显产物进入操作系统/世界生态（任务/日程/消息/回执），
//       并接收世界反馈校准"显化是否有真实效应"——防止自说自话。
// 戒律：对齐层（L6）——外显与世界协议对齐；数据来源合规（不偷盗）；
//       世界通道可信校验（防伪造反馈输入）。
// 本文件为愿景契约，实现与否待 L4/L5 落地后评估；不编译。
// =====================================================================

/// 世界回执：L5 外显被真实世界接受/执行/完成的状态。
pub struct WorldReceipt {
    pub entry_id: String,
    pub status: &'static str, // "accepted" | "executed" | "completed" | "rejected"
    pub at: u64,
}

/// 世界适配器：把显化产物投递到外部协议（任务/日程/消息…）并收回报。
/// 对齐戒律：只投递经 L5 审计的产物；来源合规（不偷盗他系统数据）；
/// 对回执做可信校验（防伪造输入成为下一次外扰）。
pub trait WorldAdapter {
    fn deliver(&self, entry: &crate_placeholder_l5::ManifestEntry) -> Result<WorldReceipt, String>;
    fn verify_receipt(&self, r: &WorldReceipt) -> bool;
}

// 占位引用说明：正式实现时引用 l5_interface 的条目类型；
// 此处不引用真实路径以免误导（本文件不编译）。
mod crate_placeholder_l5 {
    pub struct ManifestEntry {
        pub id: String,
        pub raw: String,
        pub seed: f32,
        pub lifecycle: u16,
    }
}
