#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""效力列判据（`T-052` 的可执行载体 · ★ 先只告警、不判红 · v0.328 轮立项）

【为什么】
  `T-052`「效力列」已入条文（v0.327 轮），其规则逐字要求：
    「**① 回写底图时，凡效力未达"已裁定"的内容，其行（或块）必须随带『效力列』**」
    ＋「**② 取值只有两种：🟢 已裁定（须可指裁定来源）／🟡 待裁定（须具名依据 ＋ 明写"不作为事实引用"）**」
    ＋「**③ 依据列不得留空**」＋「**④ 🟡 行不得被下游引用为事实**」
  —— 但**无判据** ⇒ 与 **`F-5`** 同族。本脚本即其落地件（B-3）。

【一句话判据】
  凡**含「效力」表头的 markdown 表**，其**数据行**须：
    **A** 每行出现 **🟢 或 🟡** 之一（不得空缺）
    **B** 🟢 行须含「**已裁定**」；🟡 行须含「**待裁定**」
    **C** 该行**依据列不得留空**（末列不得为空、不得为 `—`／`-`／`N/A`）
  违反 ⇒ **告警**（**不判红**）。

【口径（五条）】
  1. **范围**：`docs/**/*.md`（**底图**）。
  2. **表识别**：**表头行**（`|` 起首且含「效力」）＋ **紧随其后**的连续 `|` 行（**分隔行** `|---|` 跳过）。
  3. ★ **可豁免标记**：表内出现 `<!-- eff-col-skip -->` 注释的行**跳过**（**给"表内非效力行"留出口**）。
  4. **不重算机器事实**（**C18**）：只读**文本形态**；故效力＝"**格式是否合规**"，
     **不判**"🟢 是否真有裁定背书"（后者是 `T-052` 的人工守部分）。
  5. ★ **边界（如实标注）**：**"🟡 挂满 3 轮未裁 ⇒ 记缺陷"（`T-052` 追加条）本脚本 _不_ 判** ——
     其需**跨轮历史**（哪些 🟡 出现在第几轮），**当前无载体** ⇒ **漏判**。

【自检】
  `--selftest`：正反对照**七侧** ——
    ① 合规表（🟢／🟡 齐备、依据非空）⇒ **无告警**（阳性对照）
    ② **数据行缺 🟢🟡** ⇒ 告警（正例）
    ③ **🟢 行未写"已裁定"** ⇒ 告警（正例）
    ④ **🟡 行未写"待裁定"** ⇒ 告警（正例）
    ⑤ **依据列留空** ⇒ 告警（正例）
    ⑥ **含 `<!-- eff-col-skip -->` 的行被跳过** ⇒ 不告警（反例 · 豁免出口）
    ⑦ **回读真实底图**（**C18**）：真实 `docs/` 中**确实扫到含效力表的文件**，并**打印实测行数**

【用法】
  check_effect_column.py              # 实跑（**只告警**；退出码恒 0）
  check_effect_column.py --selftest    # 自检（七侧正反对照）
  check_effect_column.py --repo <path>

【触发再评估】
  告警命中**连续 N=5 轮为 0** ⇒ 再议转判红（与 `check_id_set_diff.py` 同口径）。
"""
from __future__ import annotations

import argparse
import os
import sys

SKIP_MARK = "<!-- eff-col-skip -->"
EMPTY_TOKENS = {"", "—", "-", "–", "n/a", "N/A", "无"}
GREEN, YELLOW = "🟢", "🟡"


def repo_root() -> str:
    here = os.path.dirname(os.path.abspath(__file__))
    return os.path.abspath(os.path.join(here, "..", ".."))


def iter_docs(repo: str):
    d = os.path.join(repo, "docs")
    if not os.path.isdir(d):
        return []
    out = []
    for root, _dirs, files in os.walk(d):
        for fn in sorted(files):
            if fn.endswith(".md"):
                out.append(os.path.join(root, fn))
    return out


def cells(line: str):
    s = line.strip()
    if s.startswith("|"):
        s = s[1:]
    if s.endswith("|"):
        s = s[:-1]
    return [c.strip() for c in s.split("|")]


def is_sep(line: str) -> bool:
    s = line.strip().strip("|")
    return bool(s) and set(s.replace(" ", "")) <= set("-:") | set("|")


def check_table_block(rows):
    """rows＝数据行列表；返回 alerts（不含行号前缀）"""
    alerts = []
    for ln, line in rows:
        if SKIP_MARK in line:
            continue
        cs = cells(line)
        if not cs:
            continue
        joined = " ".join(cs)
        if (GREEN not in joined) and (YELLOW not in joined):
            alerts.append("行%d：**数据行缺效力标记**（须 🟢 或 🟡）" % ln)
            continue
        if GREEN in joined and "已裁定" not in joined:
            alerts.append("行%d：**🟢 行未写「已裁定」**" % ln)
        if YELLOW in joined and "待裁定" not in joined:
            alerts.append("行%d：**🟡 行未写「待裁定」**" % ln)
        last = cs[-1].strip() if cs else ""
        if last in EMPTY_TOKENS:
            alerts.append("行%d：**依据列留空**（末列＝`%s`）" % (ln, last))
    return alerts


def evaluate_file(path: str):
    """返回 (alerts, n_tables)"""
    alerts, n_tables = [], 0
    with open(path, encoding="utf-8", errors="replace") as f:
        lines = f.read().splitlines()
    i = 0
    while i < len(lines):
        if lines[i].lstrip().startswith("|") and "效力" in lines[i]:
            n_tables += 1
            j = i + 1
            rows = []
            while j < len(lines) and lines[j].lstrip().startswith("|"):
                if not is_sep(lines[j]):
                    rows.append((j + 1, lines[j]))
                j += 1
            for a in check_table_block(rows):
                alerts.append("%s %s" % (os.path.basename(path), a))
            i = j
        else:
            i += 1
    return alerts, n_tables


def evaluate(repo: str):
    alerts, scanned = [], []
    for p in iter_docs(repo):
        a, n = evaluate_file(p)
        if n:
            scanned.append((os.path.basename(p), n))
        alerts.extend(a)
    return alerts, scanned


# ── 自检 ────────────────────────────────────────────────────────────────
GOOD = (
    "| 层 | 对内 | 对外 | 效力 | 依据 |\n"
    "|---|---|---|---|---|\n"
    "| L0 | 本觉 | 照见 | 🟢 已裁定 | 裁定① |\n"
    "| L1 | x | y | 🟡 待裁定 | 讨论稿 §2.2.5.1 |\n"
)


def selftest():
    fails = []

    def run(label, table, want_alert, tmp="/tmp/_eff.md"):
        import tempfile
        d = tempfile.mkdtemp()
        p = os.path.join(d, "t.md")
        with open(p, "w", encoding="utf-8", newline="\n") as f:
            f.write(table)
        a, n = evaluate_file(p)
        ok = (bool(a) == want_alert) and n >= 1
        print("  %s: %s%s" % (label, "PASS" if ok else "FAIL", "" if not a else "  ← %s" % a[0][:80]))
        if not ok:
            fails.append(label)

    run("侧①（阳性·合规表）", GOOD, False)
    run("侧②（正例·数据行缺 🟢🟡）",
        GOOD.replace("| L1 | x | y | 🟡 待裁定 | 讨论稿 §2.2.5.1 |\n", "| L1 | x | y | 未标 | 讨论稿 |\n"), True)
    run("侧③（正例·🟢 行未写已裁定）",
        GOOD.replace("| L0 | 本觉 | 照见 | 🟢 已裁定 | 裁定① |\n", "| L0 | 本觉 | 照见 | 🟢 | 裁定① |\n"), True)
    run("侧④（正例·🟡 行未写待裁定）",
        GOOD.replace("| L1 | x | y | 🟡 待裁定 | 讨论稿 §2.2.5.1 |\n", "| L1 | x | y | 🟡 | 讨论稿 |\n"), True)
    run("侧⑤（正例·依据列留空）",
        GOOD.replace("| L1 | x | y | 🟡 待裁定 | 讨论稿 §2.2.5.1 |\n", "| L1 | x | y | 🟡 待裁定 |  |\n"), True)
    run("侧⑥（反例·豁免出口）",
        GOOD.replace("| L1 | x | y | 🟡 待裁定 | 讨论稿 §2.2.5.1 |\n",
                     "| L1 | x | y | 说明行 " + SKIP_MARK + " |  |\n"), False)

    # 侧⑦ 回读真实底图（C18）
    r = repo_root()
    a, scanned = evaluate(r)
    ok = len(scanned) >= 1
    print("  侧⑦（对照·回读真实底图：含效力表文件 %d 份 ⇒ %s）: %s"
          % (len(scanned), ("如 " + scanned[0][0]) if scanned else "无", "PASS" if ok else "FAIL"))
    if not ok:
        fails.append("侧⑦")

    print("=" * 62)
    if fails:
        print("自检结论：FAIL %d 项 %s" % (len(fails), fails))
        return 1
    print("自检结论：PASS（七侧：4 正例 ＋ 2 反例 ＋ 1 对照）")
    return 0


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--repo", default=repo_root())
    ap.add_argument("--selftest", action="store_true")
    a = ap.parse_args()

    if a.selftest:
        return selftest()

    repo = os.path.abspath(a.repo)
    print("=" * 66)
    print("效力列判据（`T-052`）★ 先只告警，不判红")
    print("仓库根：%s" % repo)
    print("=" * 66)

    alerts, scanned = evaluate(repo)
    print("\n扫描范围：`docs/**/*.md`；**含效力表**的文件 = %d 份" % len(scanned))
    for name, n in scanned[:10]:
        print("   · %s（效力表 %d 个）" % (name, n))
    if len(scanned) > 10:
        print("   · …（其余 %d 份略）" % (len(scanned) - 10))

    print("\n" + "-" * 66)
    if alerts:
        print("⚠️ 告警 %d 条：" % len(alerts))
        for x in alerts:
            print("   - %s" % x)
    else:
        print("✅ 无告警（效力标记齐备、🟢/🟡 表述一致、依据列非空）")

    print("\n" + "=" * 66)
    print("告警合计：%d" % len(alerts))
    print("★ 本判据**先只告警、不判红**（退出码恒 0）；触发再评估＝告警命中**连续 N=5 轮为 0**。")
    print("★ 边界：只判**格式合规**；★ **「🟡 挂满 3 轮未裁 ⇒ 记缺陷」本脚本不判**（需跨轮历史，无载体）。")
    print("=" * 66)
    return 0


if __name__ == "__main__":
    sys.exit(main())
