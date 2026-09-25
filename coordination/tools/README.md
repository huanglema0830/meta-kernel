# `coordination/tools/` 判据套件索引

> **本文性质**：**已入库件**（由 2026-09-25 **23:00 夜间任务的"白天预演"**产出，经用户裁定 **apply 入库**，登记 `T-087`）。
> **来源留痕（`C19`）**：预演产物原稿＝**仓库外** `patches/nightly-p3/2026-09-25/01_tools_README.md`（`云内核-C3-017` §二 · **M 层预演**）。
> **入库时调整（如实）**：① 顶部"性质"由"补丁稿（草稿待确认）"改为"已入库件"并注明来源；② 其余内容**逐字保留**，未改写。
> **边界**：内容全部来自**对现有脚本的只读扫描**（脚本名／是否自带 `--selftest`／是否"只告警"／结论行），**未改任何脚本**。

---

## 一、套件总览（实测 16 项 ＋ 调度器）

- **判据脚本**：`coordination/tools/check_*.py` —— **共 16 个**。
- **调度器**：`run_all_checks.py`（一次跑全套，输出汇总与逐项 `rc`）。
- **排除表**：`scan_exclude.json`（扫描排除项）。
- **全跑口径**：`执行 16 个判据｜PASS(rc=0) = N｜FAIL = M`；★ 其中**只告警类**的 `rc` **恒为 0** ⇒ **其告警数须另行读输出**（`R83`）。

## 二、逐项索引（实测）

| # | 判据 | 自带 `--selftest` | **只告警（rc 恒 0）** | 结论行（节选） |
|---|---|---|---|---|
| 1 | `check_chain_visibility.py` | 有 | **是** | `FAIL n 项 [...]` |
| 2 | `check_doc_consistency.py` | 有 | 否 | `❌ 失败 n 项` |
| 3 | `check_effect_column.py` | 有 | **是** | `FAIL n 项 [...]` |
| 4 | `check_endpoint_table.py` | 有 | 否 | 口径对照（合规=0／代码多=1／表多=0／仅测试=0／锚点>0） |
| 5 | `check_id_set_diff.py` | 有 | **是** | `FAIL n 项 [...]` |
| 6 | `check_judge_gaps.py` | 有 | **是** | `FAIL n 项 [...]` |
| 7 | `check_kernel_purity.py` | 有 | 否 | `✅ 四侧均符合预期` ／ `❌ 有偏离预期的一侧` |
| 8 | `check_memory_layers.py` | 有 | **是** | `FAIL n 项 [...]` |
| 9 | `check_migration_closure.py` | 有 | 否 | `✅ 判据正反两侧均符合预期` ／ `❌ 判据存在问题` |
| 10 | `check_migration_fidelity.py` | 有 | 否 | `✅ 两侧都符合预期` ／ `❌ 判据失效` |
| 11 | `check_private_pointers.py` | 有 | 否 | `✅ 十一侧均符合预期` ／ `❌ 判据失效` |
| 12 | `check_reading_instruction.py` | 有 | 否 | （形态判据：PASS） |
| 13 | `check_report_structure.py` | 有 | **是** | `FAIL n 项 [...]` |
| 14 | `check_rewrite_pipeline.py` | 有 | **是** | `FAIL n 项 [...]` |
| 15 | `check_source_of_truth.py` | 有 | 否 | （源一致性：PASS） |
| 16 | `check_version_label.py` | 有 | **是** | `FAIL n 项 [...]` |

**统计**：**全部 16 项自带 `--selftest`**；**只告警类 8 项**（表中标"是"者），**判红类 8 项**。

## 三、怎么用

```text
# 全跑（推荐）
python coordination/tools/run_all_checks.py

# 单跑（只读某个判据的明细）
python coordination/tools/check_<name>.py

# 自检（验证判据自身"既不空转、也不误报"）
python coordination/tools/check_<name>.py --selftest
```

**读结果的两条规矩**：
1. **先看 `rc`**：`PASS(rc=0)` 不等于"零告警" —— 只告警类恒 0。
2. **再看告警数**（`R83`）：对**只告警类 8 项**必须**另行读其输出的"告警合计"**，否则会把告警读成通过。

## 四、与 CI 的接法（现状）

- CI 的 `test` job 中**逐项调用这些判据**（含门禁与只告警项）；**只告警项在 CI 中以"门禁"名义运行但 rc 恒 0**。
- **报告类判据**（报告结构／轮次号／回写五段／效力列／链可见性等）在 CI 中以**显式告警留痕**呈现。

## 五、边界（如实）

- **本地跑过**：本索引的全部字段来自**只读扫描**（脚本名、`--selftest` 存在性、只告警标记、结论行节选）。
- **CI 过**：**已**（本件入库后随 `test` job 参与 CI；**告警数不入库**＝`C20`）。
- **真实跑过**：**否** —— 本件由 **M 层"白天预演"**产出（**非真实定时触发**）。
- **未验证**：① 每项自检的**实际侧数**（本次只判"有无 `--selftest`"，未逐项实跑计数）；② 各判据**最新**的告警值（以当轮实跑为准）。
