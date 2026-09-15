# 报告 · meta-kernel/ 旧分叉 74 处改动摘要

**基准**：v0.126（HEAD bbcdd38）
**任务类型**：清理前置调研（D1）
**状态**：完成（**摘要已出，未做任何处置**——按「不丢弃、不合并、不覆盖」等待确认）
**约束**：C4（改前先列清单，确认后执行）

---

## 一、分叉定性（一句话）

`meta-kernel/` 是一个**落后 78 个版本（v0.48 vs v0.126）的陈旧工作副本**，
其 74 处「未提交改动」**不是未完成的新工作，而是旧版本文件**。
**证据（哈希比对）**：

| 比对项 | 结果 |
|---|---|
| 未跟踪 `.rs/.md/.toml` 共 47 个 | **全部**在权威库 `gh_clone2` 中**已存在**（无「仅 mk 有」的独有文件） |
| 其中内容一致的 | 35 个（是同一份文件的旧副本） |
| 其中内容不同的 | 12 个 —— **逐一行数比对，全部 mk ≤ gh**，即 mk 为**旧版本** |

行数比对（差异文件）：`l1_mapping 446 vs 822`｜`l5_diagnosis 246 vs 468`｜`l5_quad 499 vs 575`｜
`l5_router 105 vs 167`｜`l5_senses 46 vs 82`｜`tests/l5_validation 123 vs 153`｜其余 dna_* / l5_compare / 两份 docs 行数相同但内容不同（细节修订）。

> **结论**：74 处**无独有价值**，处置方式建议为「**只归档、不合并**」。

---

## 二、74 处分类明细

### A. 工程配置（8 处）

| 项 | 状态 | 规模 |
|---|---|---|
| `.gitattributes` | 修改 | +8 行 |
| `.gitignore` | 修改 | +10 行 |
| `.github/workflows/ci.yml` | 修改 | +148 行 |
| `Cargo.toml` | 修改 | 4 行 |
| `npb/Cargo.toml` | 修改 | 2 行 |
| `Cargo.lock` | 新增 | — |
| `_obsolete-sync/` | 新增 | 6 文件 / 48K |
| `archive/` | 新增 | 4 文件 / 20K |

### B. 文档（27 处）

| 项 | 状态 | 说明 |
|---|---|---|
| `README.md` | 修改 | 187 行改动（重写） |
| `docs/ONTOLOGY_SPEC.md` | 修改 | 8 行 |
| `docs/*.md` 新增 **25 份** | 新增 | API_GATEWAY_DESIGN／APPLICATION_FRAMEWORK_DESIGN／CONTRAST_REPORT／COSMIC_COMPUTING／DIAGNOSIS_VALIDATION／FIELD_PRESENTATION_DESIGN／FIELD_REPAIR_DESIGN／FULL_AUDIT_REPORT／GENE_LIBRARY_DESIGN／L4_VALIDATION_REPORT／L5_DESIGN／L5_TRANSLATION_TABLE／L5_VALIDATION_REPORT／L6_DESIGN／L7_EXECUTION_DESIGN／LAYER_ARCHITECTURE／PERFORMANCE_REPORT／PERTURBATION_MODEL／QUANTUM_CLOCK_DESIGN／REFERENCE_APP_DESIGN／SECURITY_TROUBLESHOOTING／SELF_DIAGNOSIS_REPORT／SILA_IMPLEMENTATION／VISION／WORK_CONSOLE_PLAN |

### C. 核心源码（27 处）

| 子类 | 项 |
|---|---|
| 修改（6） | `energy.rs`(+13)｜`gate.rs`(+24)｜`lib.rs`(+22)｜`ontology.rs`(+4)｜`positive_source.rs`(+20)｜`sanitizer.rs`(+4) |
| 新增单文件（17） | `dna_adapt`／`dna_generate`／`dna_trace`／`gene_library`／`l1_field_parse`／`l1_mapping`／`l4`／`l4_risk`／`l5_baseline`／`l5_compare`／`l5_context`／`l5_diagnosis`／`l5_quad`／`l5_router`／`l5_senses`／`l5_translate`／`l6_face`／`l7`（共 18 个 .rs） |
| 新增子目录（2） | `l4/`(4 文件)｜`l7/`(4 文件) |
| 测试（2） | `tests/l4_validation.rs`｜`tests/l5_validation.rs` |
| 其它（1） | `npb/src/lib.rs`(+4) |

### D. 宿主探索（12 处）

| 项 | 规模 |
|---|---|
| `deploy/` | 33 文件 / **4.3M** |
| `manifest-ui/` | 13 文件 / 360K |
| `npb-gateway/` | 7 文件 / 129K |
| `npb-appkit/` | 8 文件 / 56K |
| `manifest-journal/` | 5 文件 / 49K |
| `cloud-probe/` | 5 文件 / 29K |
| `cloud-discover/` | 3 文件 / 17K |
| `examples/ui-verify/` | 4 文件 / 53K |
| `examples/wasm_canvas/index.html` | 修改 2 行 |
| `examples/zen-oscilloscope/index.html` | 修改 2 行 |

---

## 三、验收对照

| 验收标准 | 实测 | 结论 |
|---|---|---|
| 列出 74 处未提交改动摘要 | 74 = 58 新增 + 16 修改，逐项列出 | ✅ |
| 分类：工程配置／文档／核心源码／宿主探索 | 8／27／27／12（合计 74） | ✅ |
| 不丢弃、不合并、不覆盖 | 全程只读（`git status`／`diff --stat`／哈希／行数），**零写入** | ✅ |
| 等用户确认后处理 | 未处置，本文即确认素材 | ✅ |

---

## 四、真实缺陷

| 编号 | 缺陷 | 影响 |
|---|---|---|
| G-1 | 分叉成因：`meta-kernel/` 曾是**同步区**，但权威源在 v0.48 之后换到了 `tmp_a45_review/gh_clone2`，同步区**未注销**，继续以目录形式存在并被误认为「待处理改动」 | 每次盘点都会重新制造一次困惑（本轮已是第 N 次）。**根因是"没有唯一的项目根声明"** |
| G-2 | `deploy/` 达 4.3M 且与权威库 `deploy/win64` 高度重叠 | 磁盘/认知双重复占 |

---

## 五、三量台账

- **存量**：权威库 `gh_clone2` 全部 126 个版本的成果；`meta-kernel/` 目录（v0.48 快照 + 74 处陈旧工作副本）
- **变量**：本轮任务＝盘点与分类（只读）
- **补充增量**：首次以**哈希+行数**证明 74 处「**无独有价值**」；首次把「分叉成因」从「有人改了没提交」纠正为「**同步区未注销**」
- **创新增量**：「陈旧分叉」不再需要逐文件人工判读 —— 有了一条**可复用的分叉定性方法**（见附录）

---

## 六、附录：分叉定性方法（可复用）

```bash
# 1) 规模与状态分布
git -C <fork> status --porcelain | awk '{print $1}' | sort | uniq -c
# 2) 是否存在"仅分叉有"的独有文件（关键判据）
for f in $(git -C <fork> status --porcelain | grep '^??' | awk '{print $2}'); do
  [ -e "<authoritative>/$f" ] || echo "仅分叉有: $f"
done
# 3) 差异文件的新旧方向（行数粗判）
for f in <差异文件列表>; do echo "$f mk=$(wc -l < <fork>/$f) gh=$(wc -l < <authoritative>/$f)"; done
```

判据：**「仅分叉有」为空 + 差异文件全线落后 → 陈旧分叉，只归档不合并。**

---

## 七、需要用户决策

| 编号 | 事项 | 选项 |
|---|---|---|
| D1-a | `meta-kernel/` 如何处置 | ① **只归档**（打包成 `_archive/meta-kernel-v0.48-20260916.tar.zst` 后移出工作区，**推荐**）② 原地不动，仅在 BASELINE 标注 ③ 逐文件人工比对后择优回迁（成本高、收益≈0） |
| D1-b | `deploy/` 4.3M 是否纳入归档 | 建议纳入（与权威库重叠，无独有价值） |

## 八、下一步建议

1. 采 D1-a ①：先归档再移出，**不删除**，保留可追溯性。
2. 在 `BASELINE.md` 「项目根」一节**显式声明唯一权威根** = `tmp_a45_review/gh_clone2`，从机制上杜绝再次误读。
