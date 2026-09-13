# 归档：预留接口（原 `reserved_l4_l5_l6/`）

> 归档日期：2026-09-14（v0.097）｜**原因：层号归位（D1 · 方案 A，发起人裁决）**

---

## 为什么归档

本目录原名 `reserved_l4_l5_l6/`，存放 L4/L5/L6 的**接口契约骨架**（标注 NOT COMPILED）。

随着实现推进，出现两个问题：

1. **原 L4/L5 的实现已完成**：`npb-appkit/`（应用框架）、`manifest-journal/`（参考应用）
   —— 骨架文件与实现重复。
2. **层号归位（方案 A）**：L4/L5 现专指**场域链**
   （`meta-kernel-core/src/l4/` 场域戒律判定、`l5_*.rs` 场域号脉诊断）；
   应用框架 / 参考应用**改为产品形态名，不再占用层号**。
   → 因此 `l4_interface.rs` / `l5_interface.rs` 的命名已失真。

---

## 目录内容

| 文件 | 原义 | 现状 |
|---|---|---|
| `application_framework_interface.rs`（原 `l4_interface.rs`） | 应用框架接口骨架 | 已由 `npb-appkit/` 实现 → **归档** |
| `reference_app_interface.rs`（原 `l5_interface.rs`） | 参考应用接口骨架 | 已由 `manifest-journal/` 实现 → **归档** |
| `l6_interface.rs` | WorldAdapter（世界协议对齐）愿景 | **仍未实现** → 保留为 L6 的契约参考 |

---

## 重要说明

- 这些文件**不参与编译**（无 `mod` 声明，不在任何 crate 的源码树内）。
- **归档 ≠ 废弃**：它们是设计史的一部分，也是 L6 未来实现的起点。
- 层号现状（方案 A 后）：
  - **L4 = 场域戒律拒绝层** → `meta-kernel-core/src/l4/`
  - **L5 = 场域号脉审计层** → `meta-kernel-core/src/l5_*.rs`
  - **L6 = 对齐层**（未实现，本目录 `l6_interface.rs` 为契约参考）

---

## 对应文档

- `docs/LAYER_ARCHITECTURE.md`（层定义与层号命名纪律）
- `docs/APPLICATION_FRAMEWORK_DESIGN.md`（原 `L4_APPLICATION_FRAMEWORK_DESIGN.md`）
- `docs/REFERENCE_APP_DESIGN.md`（原 `L5_REFERENCE_APP_DESIGN.md`）
- `docs/SELF_DIAGNOSIS_REPORT.md §4.2`（缺陷 D3 的原始记录）
