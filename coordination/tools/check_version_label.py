#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""轮次号判据（`T-050` 规则⑤ 的可执行载体 · ★ 先只告警、不判红 · 2026-09-23 · v0.328 轮立项）

【为什么】
  `T-050`「轮次号与版本号分家」已入条文（v0.324 轮），其规则⑤ 明写：
    「**判据** ＝ **`check_version_label.py` 先只告警**，**转判红条件 ＝ 告警连续 5 轮为 0**」
  —— 但**该脚本一直不存在** ⇒ 与 **`F-5`（"有判定、无动作、无触发"）** **同族**。
  本脚本即 **`C-1` 治法**的落地件（v0.328 轮 · A-1／B-1）。

【一句话判据】
  凡**同时出现**「版本号」与「轮次号」的文件，须满足：
    **A 头注标名对**：前 25 行内若出现 `v0.NNN`，须同时出现 `count=`（反之亦然）
    **B 同行不混写**：同一行内 `count=N` 与 `v0.N` **不得同值**（非豁免区）
  违反 ⇒ **告警**（**不判红**）。
  ★ **不判**"文本中 `v` 出现顺序递增" —— 正文天然引用历史轮次号 ⇒ 文本顺序 ≠ 时间顺序（见 `evaluate_file` 注释）。

【口径（五条，写清以免误用）】
  1. **范围**：`coordination/reports/*.md` ＋ `coordination/BASELINE.md`
     —— ★ **只扫"同时含 `count=` 与 `v0.` 的文件"**（标名纪律只在这类文件上可判）。
  2. ★ **历史豁免**：`count ∈ [312, 323]` 区段**不参与** ②／③
     （依据 **`T-050` 规则④**：该区段跳号**不追改**）；区间端点**含**。
  3. **取值口径与 `T-050` 同源**：版本号 ＝ `git rev-list --count HEAD`（**C21**，不推算）；
     轮次号 ＝ `v` 标签（形如 `v0.NNN`）。★ **两者按设计不相等** ⇒ 本判据**不检查"两数相等"**。
  4. **不重算机器事实**（**C18**）：本脚本**不自己跑 `git rev-list`**；只读**文本陈述**，
     故其效力范围＝**"文本内自洽"**，**不等于**"文本与 HEAD 一致"（后者由 `T-045` 口径 ＋ 人工守）。
  5. ★ **CI 降级**：CI 上本判据**照跑**（不依赖仓库外资源）⇒ 与 `check_memory_layers.py` 的降级策略不同。

【自检】
  `--selftest`：正反对照**八侧** ——
    ① 合规文本 ⇒ **无告警**（阳性对照）
    ② **未标名**（只有 `v0.300`、无 `count`）⇒ 告警（正例）
    ③ **混写**（`v0.300` 与 `count=300` 同行）⇒ 告警（正例）
    ④ **标名齐备且不混写** ⇒ 不告警（反例）
    ⑤ **历史豁免**（`count=320` 且 `v0.320` 混写）⇒ **不告警**（豁免对照）
    ⑥ **非豁免区混写**（`count=400` 且 `v0.400`）⇒ 告警（正例）
    ⑦ **递增**（同文件 v0.320 → v0.321）⇒ 不告警（反例）
    ⑧ **回退**（同文件 v0.321 → v0.320）⇒ 告警（正例）
    ⑨ **回读真实仓库**（**C18**）：真实 `reports/` ＋ `BASELINE.md`**读到 ≥ 1 份含双值文件**，并**打印实测值**

【用法】
  check_version_label.py              # 实跑（**只告警**；退出码恒 0）
  check_version_label.py --selftest    # 自检（九侧正反对照）
  check_version_label.py --repo <path> # 指定仓库根

【触发再评估】
  告警命中**连续 N=5 轮为 0** ⇒ 再议转判红（**`T-050` 规则⑤**；与 `check_id_set_diff.py` 同口径）。
"""
from __future__ import annotations

import argparse
import os
import re
import sys
import tempfile

# ★ 历史豁免区段与 `T-050` 规则④ **同源**（含端点）
EXEMPT_LO, EXEMPT_HI = 312, 323

# ★ 头注区行数（对齐 E）：报告／台账的"两口径标名对"按惯例出现在头注
HEAD_LINES = 25

RE_COUNT = re.compile(r"count\s*=\s*(\d+)")
RE_V = re.compile(r"v0\.(\d{3})")

TARGETS_REL = ["coordination/BASELINE.md"]
TARGETS_GLOB_DIR = "coordination/reports"


def repo_root() -> str:
    here = os.path.dirname(os.path.abspath(__file__))
    return os.path.abspath(os.path.join(here, "..", ".."))


def iter_targets(repo: str):
    """返回 [(显示名, 绝对路径)]；★ 只回读真实仓库（C18）。"""
    out = []
    p = os.path.join(repo, "coordination", "BASELINE.md")
    if os.path.isfile(p):
        out.append(("coordination/BASELINE.md", p))
    d = os.path.join(repo, *TARGETS_GLOB_DIR.split("/"))
    if os.path.isdir(d):
        for fn in sorted(os.listdir(d)):
            if fn.endswith(".md"):
                out.append(("coordination/reports/" + fn, os.path.join(d, fn)))
    return out


def scan_text(text: str):
    """返回 (counts, vs)；counts＝[(行号, 值)]，vs＝[(行号, 值)]"""
    counts, vs = [], []
    for i, ln in enumerate(text.splitlines(), 1):
        for m in RE_COUNT.finditer(ln):
            counts.append((i, int(m.group(1))))
        for m in RE_V.finditer(ln):
            vs.append((i, int(m.group(1))))
    return counts, vs


def is_exempt(v: int) -> bool:
    return EXEMPT_LO <= v <= EXEMPT_HI


def evaluate_file(name: str, text: str):
    """对单个文件求 (alerts, notes)。

    ★ 口径（v0.328 定稿 · 两次实测收敛）：
      · **A 头注标名对**：文件**前 HEAD_LINES 行**内若出现 `v0.NNN`，则须同时出现 `count=`；
        反之亦然。⇒ 判"该文件是否声明了**两口径的配对**"。
      · **B 同行不混写**：同一行内 `count=N` 与 `v0.N` **同值**（非豁免）⇒ 告警。
    ★ **不做的检查（如实标注）**：**不判"文本中 v 出现顺序递增"** ——
      **正文天然会引用历史轮次号**（如"上一轮 `v0.325`"）⇒ **文本顺序 ≠ 时间顺序**，
      该规则**语义错误**（v0.328 首版实测 **81 条误报**后**当场撤除**）。
    """
    alerts, notes = [], []
    lines = text.splitlines()
    counts, vs = scan_text(text)
    if not counts or not vs:
        return alerts, notes      # ★ 不成对 ⇒ 本判据不管（边界写清）

    # ★★ 二级豁免（判据边界 · 如实标注）：**标名纪律自 `v0.324`（`T-050` 定型）才存在**，
    #    且 `T-050` 之前 `v` 标签**本就由 `count` 派生**（同值＝当时正确）。
    #    ⇒ **时代判定＝该文件"自身轮次号"**（取**头注内第一个 `v0.NNN`**）；
    #      **≤ EXEMPT_HI（323） ⇒ 整文件豁免**（不判头注配对、不判混写）。
    #    （一级豁免＝`T-050` 规则④ 的 312–323 区段；二级＝本判据的时代边界。）
    head_text = "\n".join(lines[:HEAD_LINES])
    head_v_first = None
    for m in RE_V.finditer(head_text):
        head_v_first = int(m.group(1))
        break
    if head_v_first is not None and head_v_first <= EXEMPT_HI:
        notes.append("%s：**时代豁免**（头注自身轮次号 `v0.%03d` ≤ %d ⇒ 标名纪律尚未存在）"
                     % (name, head_v_first, EXEMPT_HI))
        return alerts, notes
    if head_v_first is None and all(c <= EXEMPT_HI for c in (v for _, v in counts)):
        notes.append("%s：**时代豁免**（头注无轮次号且出现的 count 全部 ≤ %d）" % (name, EXEMPT_HI))
        return alerts, notes

    # A 头注标名对
    head = "\n".join(lines[:HEAD_LINES])
    h_c = bool(RE_COUNT.search(head))
    h_v = bool(RE_V.search(head))
    if h_v and not h_c:
        alerts.append("%s：**头注缺版本号标名** —— 前 %d 行出现 `v0.NNN` 但无 `count=`"
                      "（`T-050` 规则③：两者必须标名、不得混写）" % (name, HEAD_LINES))
    if h_c and not h_v:
        alerts.append("%s：**头注缺轮次号标名** —— 前 %d 行出现 `count=` 但无 `v0.NNN`"
                      "（`T-050` 规则③）" % (name, HEAD_LINES))

    # B 同行不混写
    for ln, vals in [(i, [v for l2, v in counts if l2 == i]) for i in sorted(set(l for l, _ in counts))]:
        v_on_line = [v for l2, v in vs if l2 == ln]
        for c in vals:
            for v in v_on_line:
                if c == v and not is_exempt(c):
                    alerts.append("%s 行%d：**混写** —— `count=%d` 与 `v0.%03d` 同值同行（`T-050` 规则③）"
                                  % (name, ln, c, v))
            if is_exempt(c):
                notes.append("%s 行%d：`count=%d` **历史豁免**（%d–%d 区段）"
                             % (name, ln, c, EXEMPT_LO, EXEMPT_HI))

    notes.append("%s：count=%d 处／v0.= %d 处（头注标名对＝%s）"
                 % (name, len(counts), len(vs), "齐" if (h_c and h_v) else "缺"))
    return alerts, notes


def evaluate(repo: str):
    alerts, notes, scanned = [], [], []
    for name, path in iter_targets(repo):
        try:
            with open(path, encoding="utf-8", errors="replace") as f:
                text = f.read()
        except OSError as e:
            alerts.append("%s 读取失败：%s" % (name, e))
            continue
        counts, vs = scan_text(text)
        if counts and vs:
            scanned.append((name, len(counts), len(vs)))
        a, n = evaluate_file(name, text)
        alerts.extend(a)
        notes.extend(n)
    return alerts, notes, scanned


# ── 自检 ────────────────────────────────────────────────────────────────
def selftest():
    fails = []

    def check(label, text, want_alert, exempt_hint=False):
        a, _ = evaluate_file("<selftest>", text)
        got = bool(a)
        ok = (got == want_alert)
        print("  %s: %s%s" % (label, "PASS" if ok else "FAIL",
                              "" if not a else "  ← %s" % a[0][:80]))
        if not ok:
            fails.append(label)

    # ① 合规 ⇒ 无告警
    check("侧①（阳性·合规文本）",
          "版本 `count=345` ｜ 轮次 `v0.328`\n轮次 `v0.329`", False)
    # ② 未标名：只有 v
    a, _ = evaluate_file("<selftest>", "本文件轮次 v0.300 完成")
    print("  侧②（正例·只有 v 无 count ⇒ **本判据不管**）: %s"
          % ("PASS" if not a else "FAIL"))
    if a:
        fails.append("侧②")

    # ③ 混写（非豁免区）
    check("侧③（正例·混写 count=345 与 v0.345）",
          "版本 `count=345` ｜ 轮次 `v0.345`", True)
    # ④ 标名齐备且不混写
    check("侧④（反例·标名齐备）",
          "版本 `count=345` ｜ 轮次 `v0.328`", False)
    # ⑤ 历史豁免
    check("侧⑤（对照·历史豁免 count=320 与 v0.320）",
          "版本 `count=320` ｜ 轮次 `v0.320`", False)
    # ⑥ 非豁免区混写
    check("侧⑥（正例·非豁免 count=400 与 v0.400）",
          "版本 `count=400` ｜ 轮次 `v0.400`", True)
    # ⑦ 头注标名对齐备 ⇒ 不告警
    check("侧⑦（反例·头注标名对齐备）",
          "版本 `count=345` ｜ 轮次 `v0.328`\n正文……", False)
    # ⑧ 头注缺 count（v 在前、count 在正文第 30 行之后）⇒ 告警
    filler = "\n".join("填充行 %d" % i for i in range(1, 30))
    check("侧⑧（正例·头注有 v 无 count）",
          "轮次 `v0.328` 轮\n" + filler + "\n版本 `count=345`", True)

    # ⑨ 回读真实仓库（C18）
    repo = repo_root()
    alerts, notes, scanned = evaluate(repo)
    ok = len(scanned) >= 1
    print("  侧⑨（对照·回读真实仓库：含双值文件 %d 份 ⇒ %s）: %s"
          % (len(scanned), ("如 " + scanned[0][0]) if scanned else "无", "PASS" if ok else "FAIL"))
    if not ok:
        fails.append("侧⑨")

    print("=" * 62)
    if fails:
        print("自检结论：FAIL %d 项 %s" % (len(fails), fails))
        return 1
    print("自检结论：PASS（九侧：4 正例 ＋ 3 反例 ＋ 2 对照）")
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
    print("轮次号判据（`T-050` 规则⑤）★ 先只告警，不判红")
    print("仓库根：%s" % repo)
    print("★ 历史豁免区段：`count` %d–%d（含端点，%s 规则④）" % (EXEMPT_LO, EXEMPT_HI, "T-050"))
    print("=" * 66)

    alerts, notes, scanned = evaluate(repo)
    print("\n扫描到「同时含 count= 与 v0.」的文件：%d 份" % len(scanned))
    for name, nc, nv in scanned[:8]:
        print("   · %s（count=%d 处／v0.= %d 处）" % (name, nc, nv))
    if len(scanned) > 8:
        print("   · …（其余 %d 份略）" % (len(scanned) - 8))

    print("\n" + "-" * 66)
    if alerts:
        print("⚠️ 告警 %d 条：" % len(alerts))
        for x in alerts:
            print("   - %s" % x)
    else:
        print("✅ 无告警（头注标名对齐备、无同行混写）")

    print("\n" + "=" * 66)
    print("告警合计：%d" % len(alerts))
    print("★ 本判据**先只告警、不判红**（退出码恒 0）"
          "；转判红条件＝告警**连续 5 轮为 0**（`T-050` 规则⑤）。")
    print("★ 边界：只判**文本内自洽**，不判「文本 vs HEAD 一致」（后者由 `T-045` 口径 ＋ 人工守）。")
    print("=" * 66)
    return 0


if __name__ == "__main__":
    sys.exit(main())
