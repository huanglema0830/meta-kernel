#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""判据缺口巡检（★ 先只告警、不判红 · v0.329 轮立项 · 治 `F-5` 的**批量**治法）

【为什么】
  `F-5`＝「新判据**有判定、无动作、无触发**」；其**更普遍的形态**是：
  **治理文件里的条文自称「判据：暂无／无」** ⇒ 该条文**只有纪律、没有机器兜底**。
  历轮治法都是**逐个治**（发现一条 → 立一个判据）；**v0.328 轮实证可批量治**：
  该轮一次性把 **`T-050` 规则⑤／`T-051`／`T-052` 三条**（三条**均自称"判据：无"**）
  同轮变成 **3 个判据** ⇒ **识别入口＝"扫条文里的『判据：无』字样"**（可机器化）。

【一句话判据】
  扫**治理文件**（`coordination/` 顶层 5 份：`CHARTER`／`CONSTRAINTS`／`TEMPLATES`／`TERMS`／`ROADMAP`），
  收集所有形如 **`判据：无`／`判据：暂无`** 的行 ⇒ 形成**缺口清单**；
  与**基线快照**（`coordination/security/judge_gaps_baseline.txt`）比对：
    **新增缺口** ⇒ **告警**（＝**新条文又只立了纪律、没立判据**）；
    **缺口消除** ⇒ **提示更新基线**（**不告警** —— 这是好事）。
  违反 ⇒ **告警**（**不判红**）。

【口径（六条）】
  1. **范围**：**只扫 `coordination/` 顶层 5 份治理文件** ——
     ★ **不含** `BASELINE.md`（台账，**不是治理条文**）／`reports/`／`docs/`；
     ★ 也**不含** `coordination/tools/`（**避免把判据自己的文档算作缺口**）。
  2. ★ **自述剔除**（同源先例＝`check_id_set_diff` 剔除"自述告警段"；
     **教训来源＝自指污染第 12 例**）：**含本判据文件名**的行**不参与**。
  3. **匹配式**：`判据` ＋ **可选冒号** ＋ **`无` 或 `暂无`**（**同行**）。
  4. **基线口径**：基线文件为**行文本快照**（一行一条，含 `文件｜行文本`）
     —— ★ **不用行号**（行号会随编辑漂移 ⇒ 假增假减）。
  5. **更新方式**：`--update-baseline` **显式更新**（同 `check_source_of_truth.py` 的判据 F 口径）。
  6. **不重算机器事实**（**C18**）：只读**文本**；故效力＝"**缺口清单是否变多**"，
     **不判**"该不该立判据"（后者是发起人的裁定）。

【自检】
  `--selftest`：正反对照**六侧** ——
    ① 清单与基线**相同** ⇒ **无告警**（阳性对照）
    ② **新增一条**缺口 ⇒ 告警（正例）
    ③ **减少一条**缺口 ⇒ **不告警**（反例 · 消除是好事）
    ④ ★ **自述剔除**：含**本判据文件名**的行**不进清单**（反例 · 治**自指污染**）
    ⑤ **匹配式**：`判据：无`／`判据：暂无`／`判据: 无` 均命中；`判据 有` 不命中（正例 ＋ 反例）
    ⑥ **回读真实治理文件**（**C18**）：**确实扫到 ≥ 1 条缺口**，并**打印清单前若干条**

【用法】
  check_judge_gaps.py                    # 实跑（**只告警**；退出码恒 0）
  check_judge_gaps.py --update-baseline   # 显式更新基线（缺口消除后用）
  check_judge_gaps.py --selftest          # 自检（六侧正反对照）
  check_judge_gaps.py --repo <path>

【触发再评估】
  告警命中**连续 N=5 轮为 0** ⇒ 再议转判红（与 `check_id_set_diff.py` 同口径）。
"""
from __future__ import annotations

import argparse
import os
import re
import sys

GOV = ["CHARTER.md", "CONSTRAINTS.md", "TEMPLATES.md", "TERMS.md", "ROADMAP.md"]

RE_GAP = re.compile(r"判据[：:]?\s*(暂无|无)")
SELF_NAME = "check_judge_gaps"
BASELINE_REL = "coordination/security/judge_gaps_baseline.txt"


def repo_root() -> str:
    here = os.path.dirname(os.path.abspath(__file__))
    return os.path.abspath(os.path.join(here, "..", ".."))


def strip_self_mention(text: str) -> str:
    """★ **自述剔除**：含本判据文件名的行不参与（**自指污染**防治 · `T-049`）。"""
    return "\n".join(ln for ln in text.splitlines() if SELF_NAME not in ln)


def collect(repo: str):
    """返回 [(相对路径, 行文本 stripped)]；★ 去重后按 (路径, 文本) 排序。"""
    out = []
    cd = os.path.join(repo, "coordination")
    for fn in GOV:
        p = os.path.join(cd, fn)
        if not os.path.isfile(p):
            continue
        with open(p, encoding="utf-8", errors="replace") as f:
            for ln in f.read().splitlines():
                if SELF_NAME in ln:            # ★ 自述剔除
                    continue
                if RE_GAP.search(ln):
                    out.append(("coordination/" + fn, ln.strip()))
    seen, uniq = set(), []
    for it in sorted(set(out)):
        if it in seen:
            continue
        seen.add(it)
        uniq.append(it)
    return uniq


def load_baseline(repo: str):
    p = os.path.join(repo, *BASELINE_REL.split("/"))
    if not os.path.isfile(p):
        return None
    with open(p, encoding="utf-8", errors="replace") as f:
        return sorted({ln.strip() for ln in f.read().splitlines() if ln.strip()})


def write_baseline(repo: str, items):
    p = os.path.join(repo, *BASELINE_REL.split("/"))
    os.makedirs(os.path.dirname(p), exist_ok=True)
    with open(p, "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join("%s｜%s" % (a, b) for a, b in items) + "\n")
    return p


def as_lines(items):
    return sorted("%s｜%s" % (a, b) for a, b in items)


def compare(cur_lines, base_lines):
    """返回 (alerts, notes)"""
    alerts, notes = [], []
    if base_lines is None:
        alerts.append("**基线不存在** ⇒ 缺口清单**无对照**（**空转不得与通过同值**）；"
                      "请先 `--update-baseline` 建立基线")
        return alerts, notes
    added = sorted(set(cur_lines) - set(base_lines))
    gone = sorted(set(base_lines) - set(cur_lines))
    for a in added:
        alerts.append("**新增缺口**：%s" % a[:150])
    for g in gone:
        notes.append("★ **缺口已消除**（请更新基线）：%s" % g[:150])
    notes.append("缺口清单：当前 %d 条 ／ 基线 %d 条" % (len(cur_lines), len(base_lines)))
    return alerts, notes


# ── 自检 ────────────────────────────────────────────────────────────────
def selftest():
    fails = []

    def chk(label, ok, extra=""):
        print("  %s: %s%s" % (label, "PASS" if ok else "FAIL", ("  ← " + extra) if extra else ""))
        if not ok:
            fails.append(label)

    # 侧① 相同 ⇒ 无告警
    a, _ = compare(["x｜判据：无"], ["x｜判据：无"])
    chk("侧①（阳性·清单==基线 ⇒ 无告警）", not a)
    # 侧② 新增 ⇒ 告警
    a, _ = compare(["x｜判据：无", "y｜判据：暂无"], ["x｜判据：无"])
    chk("侧②（正例·新增一条 ⇒ 告警）", bool(a))
    # 侧③ 减少 ⇒ 不告警
    a, n = compare(["x｜判据：无"], ["x｜判据：无", "y｜判据：暂无"])
    chk("侧③（反例·减少 ⇒ 不告警 ＋ 提示）", (not a) and bool(n))
    # 侧④ 自述剔除
    txt = "T-999 新条文 判据：无\n" + SELF_NAME + ".py 自身文档也写了 判据：无\n"
    kept = [ln for ln in strip_self_mention(txt).splitlines() if RE_GAP.search(ln)]
    chk("侧④（反例·自述剔除 ⇒ 只剩 1 条）", len(kept) == 1, "剩 %d 条" % len(kept))
    # 侧⑤ 匹配式
    hit = all(RE_GAP.search(s) for s in ["判据：无", "判据：暂无", "判据: 无", "判据无"])
    miss = not RE_GAP.search("判据：有")
    chk("侧⑤（正例＋反例·匹配式）", hit and miss)
    # 侧⑥ 回读真实治理文件（C18）
    r = repo_root()
    items = collect(r)
    ok = len(items) >= 1
    print("  侧⑥（对照·回读真实治理文件：缺口 %d 条 ⇒ 首条＝%s）: %s"
          % (len(items), ("%s｜%s" % items[0])[:60] if items else "无", "PASS" if ok else "FAIL"))
    if not ok:
        fails.append("侧⑥")

    print("=" * 62)
    if fails:
        print("自检结论：FAIL %d 项 %s" % (len(fails), fails))
        return 1
    print("自检结论：PASS（六侧：2 正例 ＋ 3 反例 ＋ 1 对照）")
    return 0


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--repo", default=repo_root())
    ap.add_argument("--update-baseline", action="store_true")
    ap.add_argument("--selftest", action="store_true")
    a = ap.parse_args()

    if a.selftest:
        return selftest()

    repo = os.path.abspath(a.repo)
    print("=" * 66)
    print("判据缺口巡检（治 `F-5` 的批量治法）★ 先只告警，不判红")
    print("仓库根：%s" % repo)
    print("扫描范围：coordination/ 顶层 %d 份治理文件（%s）" % (len(GOV), "／".join(GOV)))
    print("★ 自述剔除：含 `%s` 的行不参与" % SELF_NAME)
    print("=" * 66)

    items = collect(repo)
    cur = as_lines(items)
    print("\n**缺口清单（条文自称「判据：无」）＝ %d 条**" % len(cur))
    for x in cur[:12]:
        print("   · %s" % x[:150])
    if len(cur) > 12:
        print("   · …（其余 %d 条略）" % (len(cur) - 12))

    if a.update_baseline:
        p = write_baseline(repo, items)
        print("\n★ 基线已写入：%s（%d 条）" % (p, len(items)))
        return 0

    base = load_baseline(repo)
    alerts, notes = compare(cur, base)
    print("")
    for n in notes:
        print("   %s" % n)

    print("\n" + "-" * 66)
    if alerts:
        print("⚠️ 告警 %d 条：" % len(alerts))
        for x in alerts:
            print("   - %s" % x)
    else:
        print("✅ 无告警（缺口清单未新增；已消除项请更新基线）")

    print("\n" + "=" * 66)
    print("告警合计：%d" % len(alerts))
    print("★ 本判据**先只告警、不判红**（退出码恒 0）；触发再评估＝告警命中**连续 N=5 轮为 0**。")
    print("★ 边界：只判**清单是否变多**，不判「该不该立判据」（那是发起人的裁定）。")
    print("=" * 66)
    return 0


if __name__ == "__main__":
    sys.exit(main())
