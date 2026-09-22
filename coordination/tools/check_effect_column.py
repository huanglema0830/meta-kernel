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
  ★ **D 时效（`T-053`＝`§二十二 22.5` 的机器载体 · v0.329 轮接入）**：
    在**效力语境**内的 🟡（＝**效力表数据行**内的 🟡，或**同行含「待复核」**的 🟡）
    **须带「首次标记：v0.NNN 轮」**（缺 ⇒ 告警）；且 **`当前轮次 − 首次标记 > 3` ⇒ 告警**（挂满 3 轮未裁）。
  违反 ⇒ **告警**（**不判红**）。

【口径（六条）】
  1. **范围**：`docs/**/*.md`（**底图**）。
  2. **表识别**：**表头行**（`|` 起首且含「效力」）＋ **紧随其后**的连续 `|` 行（**分隔行** `|---|` 跳过）。
  3. ★ **可豁免标记**：表内出现 `<!-- eff-col-skip -->` 注释的行**跳过**（**给"表内非效力行"留出口**）。
  4. **当前轮次** ＝ `coordination/BASELINE.md` **最后一个顶层节**标题里的 `v0.NNN`；
     **读不到 ⇒ 只判"有无标记"、不判"挂几轮"**（**如实降级，不伪装**）。
  5. **不重算机器事实**（**C18**）：只读**文本形态**；故效力＝"**格式 ＋ 时效**"，
     **不判**"🟢 是否真有裁定背书"（后者是 `T-052` 的人工守部分）。
  6. ★ **边界（如实标注）**：**底图中 🟡 的"进度类"用法**（如 `🟡 部分`／`🟡 60%`）**不属效力语境**
     ⇒ **不参与**（v0.329 实测：底图另有 **26 处**此类用法，一并纳入会**大面积误报**）。

【自检】
  `--selftest`：正反对照**十侧** ——
    ① 合规表（🟢／🟡 齐备、依据非空、🟡 带首次标记）⇒ **无告警**（阳性对照）
    ② **数据行缺 🟢🟡** ⇒ 告警（正例）
    ③ **🟢 行未写"已裁定"** ⇒ 告警（正例）
    ④ **🟡 行未写"待裁定"** ⇒ 告警（正例）
    ⑤ **依据列留空** ⇒ 告警（正例）
    ⑥ **含 `<!-- eff-col-skip -->` 的行被跳过** ⇒ 不告警（反例 · 豁免出口）
    ⑦ **回读真实底图**（**C18**）：真实 `docs/` 中**确实扫到含效力表的文件**，并**打印实测行数与当前轮次**
    ⑧ **🟡 缺首次标记** ⇒ 告警（正例 · `T-053`）
    ⑨ **🟡 挂满 >3 轮**（首次 `v0.100` ／当前 `v0.328`）⇒ 告警（正例 · `T-053`）
    ⑩ **🟡 未满 3 轮**（首次 `v0.326` ／当前 `v0.328`）⇒ 不告警（反例 · 边界另一侧）

【用法】
  check_effect_column.py              # 实跑（**只告警**；退出码恒 0）
  check_effect_column.py --selftest    # 自检（十侧正反对照）
  check_effect_column.py --repo <path>

【触发再评估】
  告警命中**连续 N=5 轮为 0** ⇒ 再议转判红（与 `check_id_set_diff.py` 同口径）。
"""
from __future__ import annotations

import argparse
import os
import re
import sys

SKIP_MARK = "<!-- eff-col-skip -->"
EMPTY_TOKENS = {"", "—", "-", "–", "n/a", "N/A", "无"}
GREEN, YELLOW = "🟢", "🟡"

# ★ `T-053`（`§二十二 22.5`）的机器载体：🟡 挂满 3 轮未裁 ⇒ 记缺陷
RE_FIRST = re.compile(r"首次标记[：:\s]*v0\.(\d{3})")
RE_CUR = re.compile(r"v0\.(\d{3})")
AGING_LIMIT = 3   # 满 3 轮（含起点轮为第 1 轮）；★ 与 `T-053` 22.5 同源


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


def current_round(repo: str):
    """当前轮次号 ＝ `coordination/BASELINE.md` **最后一个顶层节**标题里的 `v0.NNN`。
    ★ 口径与 `T-050` 同源（轮次号＝`v` 标签）；**读不到 ⇒ 返回 None** ⇒ **时效检查只判"有无标记"、不判"挂几轮"**
    （**如实降级，不伪装**）。
    """
    p = os.path.join(repo, "coordination", "BASELINE.md")
    if not os.path.isfile(p):
        return None
    cur = None
    with open(p, encoding="utf-8", errors="replace") as f:
        for ln in f:
            if ln.startswith("## ") and not ln.startswith("### "):
                m = RE_CUR.search(ln)
                if m:
                    cur = int(m.group(1))
    return cur


def aging_alerts(lines, cur, row_lines):
    """★ `T-053`（`§二十二 22.5`）的时效检查 —— 治"🟡 无限期挂起"。

    **在范围内的 🟡**（口径写清）：
      (a) **效力表数据行**内的 🟡；或
      (b) 同行含**「待复核」**的 🟡（＝ `T-052` **块级标注**的形态）。
    **不在范围（如实标注的判据边界）**：底图中 🟡 的**其他用法** —— **进度／完成度标记**
      （如 `🟡 部分`／`🟡 60%`），**不属效力语境** ⇒ **不参与**
      （v0.329 实测：底图另有 **26 处**此类用法，若一并纳入会**大面积误报**）。
    要求：每处在范围内 🟡 **须带「首次标记：v0.NNN」**；缺 ⇒ **告警**；
      **`cur - first > 3`** ⇒ **告警**（挂满 3 轮未裁）。
    """
    out = []
    for ln, line in enumerate(lines, 1):
        if YELLOW not in line or SKIP_MARK in line:
            continue
        if not (ln in row_lines or "待复核" in line):
            continue
        m = RE_FIRST.search(line)
        if not m:
            out.append("行%d：**🟡 缺「首次标记轮次」**（`T-053` 22.5 要求；格式 `首次标记：v0.NNN 轮`）" % ln)
            continue
        first = int(m.group(1))
        if cur is not None and (cur - first) > AGING_LIMIT:
            out.append("行%d：**🟡 挂满 %d 轮未裁**（首次标记 `v0.%03d` ／ 当前 `v0.%03d` ＞ 上限 %d 轮）"
                       % (ln, cur - first, first, cur, AGING_LIMIT))
    return out


def evaluate_file(path: str, cur=None):
    """返回 (alerts, n_tables)"""
    alerts, n_tables = [], 0
    with open(path, encoding="utf-8", errors="replace") as f:
        lines = f.read().splitlines()
    i = 0
    row_lines = set()
    while i < len(lines):
        if lines[i].lstrip().startswith("|") and "效力" in lines[i]:
            n_tables += 1
            j = i + 1
            rows = []
            while j < len(lines) and lines[j].lstrip().startswith("|"):
                if not is_sep(lines[j]):
                    rows.append((j + 1, lines[j]))
                    row_lines.add(j + 1)
                j += 1
            for a in check_table_block(rows):
                alerts.append("%s %s" % (os.path.basename(path), a))
            i = j
        else:
            i += 1
    for a in aging_alerts(lines, cur, row_lines):
        alerts.append("%s %s" % (os.path.basename(path), a))
    return alerts, n_tables


def evaluate(repo: str):
    alerts, scanned = [], []
    cur = current_round(repo)
    for p in iter_docs(repo):
        a, n = evaluate_file(p, cur=cur)
        if n:
            scanned.append((os.path.basename(p), n))
        alerts.extend(a)
    return alerts, scanned, cur


# ── 自检 ────────────────────────────────────────────────────────────────
GOOD = (
    "| 层 | 对内 | 对外 | 效力 | 依据 |\n"
    "|---|---|---|---|---|\n"
    "| L0 | 本觉 | 照见 | 🟢 已裁定 | 裁定① |\n"
    "| L1 | x | y | 🟡 待裁定（首次标记：v0.328 轮） | 讨论稿 §2.2.5.1 |\n"
)
YROW = "| L1 | x | y | 🟡 待裁定（首次标记：v0.328 轮） | 讨论稿 §2.2.5.1 |\n"


def selftest():
    import tempfile

    fails = []
    holder = {}

    def write(table):
        d = tempfile.mkdtemp()
        p = os.path.join(d, "t.md")
        with open(p, "w", encoding="utf-8", newline="\n") as f:
            f.write(table)
        return p

    def run(label, table, want_alert, cur=None, want_sub=None):
        p = write(table)
        a, n = evaluate_file(p, cur=cur)
        got = bool(a)
        sub_ok = True if want_sub is None else any(want_sub in x for x in a)
        ok = (got == want_alert) and n >= 1 and sub_ok
        print("  %s: %s%s" % (label, "PASS" if ok else "FAIL", "" if not a else "  ← %s" % a[0][:80]))
        if not ok:
            fails.append(label)

    run("侧①（阳性·合规表）", GOOD, False)
    run("侧②（正例·数据行缺 🟢🟡）", GOOD.replace(YROW, "| L1 | x | y | 未标 | 讨论稿 |\n"), True)
    run("侧③（正例·🟢 行未写已裁定）",
        GOOD.replace("| L0 | 本觉 | 照见 | 🟢 已裁定 | 裁定① |\n", "| L0 | 本觉 | 照见 | 🟢 | 裁定① |\n"), True)
    run("侧④（正例·🟡 行未写待裁定）",
        GOOD.replace(YROW, "| L1 | x | y | 🟡 | 讨论稿 |\n"), True)
    run("侧⑤（正例·依据列留空）",
        GOOD.replace(YROW, "| L1 | x | y | 🟡 待裁定 |  |\n"), True)
    run("侧⑥（反例·豁免出口）",
        GOOD.replace(YROW, "| L1 | x | y | 说明行 " + SKIP_MARK + " |  |\n"), False)
    # ★ `T-053` 时效三侧
    run("侧⑧（正例·🟡 缺首次标记）",
        GOOD.replace(YROW, "| L1 | x | y | 🟡 待裁定 | 讨论稿 |\n"), True, want_sub="缺「首次标记轮次」")
    run("侧⑨（正例·挂满 >3 轮）",
        GOOD.replace(YROW, YROW.replace("v0.328", "v0.100")), True, cur=328, want_sub="挂满")
    run("侧⑩（反例·未满 3 轮）",
        GOOD.replace(YROW, YROW.replace("v0.328", "v0.326")), False, cur=328)

    # 侧⑦ 回读真实底图（C18）
    r = repo_root()
    a, scanned, cur = evaluate(r)
    ok = len(scanned) >= 1
    print("  侧⑦（对照·回读真实底图：含效力表文件 %d 份 ／ 当前轮次 %s）: %s"
          % (len(scanned), ("v0.%03d" % cur) if cur else "未读到", "PASS" if ok else "FAIL"))
    if not ok:
        fails.append("侧⑦")

    print("=" * 62)
    if fails:
        print("自检结论：FAIL %d 项 %s" % (len(fails), fails))
        return 1
    print("自检结论：PASS（十侧：5 正例 ＋ 2 反例 ＋ 3 对照）")
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

    alerts, scanned, cur = evaluate(repo)
    print("\n扫描范围：`docs/**/*.md`；**含效力表**的文件 = %d 份" % len(scanned))
    for name, n in scanned[:10]:
        print("   · %s（效力表 %d 个）" % (name, n))
    if len(scanned) > 10:
        print("   · …（其余 %d 份略）" % (len(scanned) - 10))
    print("★ 时效检查（`T-053` 22.5）：**当前轮次 %s** ／ **上限 %d 轮**"
          % (("v0.%03d" % cur) if cur else "未读到（⇒ 只判有无标记）", AGING_LIMIT))

    print("\n" + "-" * 66)
    if alerts:
        print("⚠️ 告警 %d 条：" % len(alerts))
        for x in alerts:
            print("   - %s" % x)
    else:
        print("✅ 无告警（效力标记齐备、🟢/🟡 表述一致、依据列非空、🟡 未超时效）")

    print("\n" + "=" * 66)
    print("告警合计：%d" % len(alerts))
    print("★ 本判据**先只告警、不判红**（退出码恒 0）；触发再评估＝告警命中**连续 N=5 轮为 0**。")
    print("★ 边界：只判**格式 ＋ 时效**；★ 底图中 🟡 的**进度类用法**（如 `🟡 部分`）**不属效力语境、不参与**。")
    print("=" * 66)
    return 0


if __name__ == "__main__":
    sys.exit(main())
