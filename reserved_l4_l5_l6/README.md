// =====================================================================
// reserved_l4_l5_l6/README.md — L4-L6 预留接口说明
// =====================================================================
// 目的：L1-L3 已有实现（meta-kernel-core / npb / npb-gateway）。
//       L4（框架·拒绝层 npb-appkit）、L5（应用·审计层 Manifest Journal）、
//       L6（对齐层 WorldAdapter）为设计已定稿/愿景层——本目录提供
//       【预留接口文件】（签名/契约骨架 + 分层戒律标注），
//       【不编译、不参与 workspace、不实现】。
//
// 用法：
//   - 本目录不加入 Cargo workspace members（保持 CI/编译面不变）；
//   - 接口内容来自 docs/L4_APPLICATION_FRAMEWORK_DESIGN v1.0、
//     docs/L5_REFERENCE_APP_DESIGN v1.0、docs/LAYER_ARCHITECTURE、
//     docs/SILA_IMPLEMENTATION、docs/PERTURBATION_MODEL；
//   - 实现轮（A 类流水线）激活时：把对应接口移入正式 crate（npb-appkit /
//     manifest-journal / world-adapter）并补 #[cfg(test)] 与 CI 测试。
//
// 激活状态（阶段 2 · 2026-09-08）：
//   [ACTIVATED] l4_interface.rs → npb-appkit/（workspace 正式 crate：
//       LifecycleEngine 纯函数状态机 + Namer/Speaker + EventPipe + httpc，含单测）
//   [ACTIVATED] l5_interface.rs → manifest-journal/（L5 最小版：seed_of 指纹种子、
//       JournalSession 端到端 + tests/e2e_manifest.rs 验收五条）
//   [RESERVED]  l6_interface.rs（L6 对齐层 WorldAdapter —— 愿景，待 L4/L5 落地后评估）
//
// 文件：
//   l4_interface.rs  — L4 框架契约历史快照（激活指针见 npb-appkit）—— 拒绝层戒律锚点
//   l5_interface.rs  — L5 Manifest Journal 契约历史快照（激活指针见 manifest-journal）——
//                      审计层戒律锚点
//   l6_interface.rs  — L6 对齐层愿景契约（WorldAdapter / 世界反馈校验）——
//                      对齐层戒律锚点
