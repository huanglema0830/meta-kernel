#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""MEMORY 三层指针机制判据（★ 先只告警、不判红 · 2026-09-22 · v0.319 轮立项）

【为什么】
  `CHARTER.md` **机制 30**（＝`TEMPLATES.md` **`T-046`**）规定记忆按「**调用／索引／内容**」三层分置，
  并给出**两个可机器化的上限**：**调用层 < 1 KB**、**索引层 < 4 KB**。
  但截至 v0.318，机制 30 条目自身写着「**机器判据：暂无**」⇒ **上限全靠人守**；
  且 **v0.318 一轮之内三次触线**（调用层一度 **1127 B**／索引层一度 **4398 B**／内容层一度 **33 153 B**）
  ⇒ **人工守不住，须机器守**。

【一句话判据】
  工作区记忆目录 `.workbuddy/memory/` 须满足：
    ① **调用层** `MEMORY.md`       **< 1024 B**
    ② **索引层** `MEMORY_INDEX.md` **< 4096 B**
    ③ **内容层** 4 份（`ironlaws.md`／`env.md`／`state.md`／`rounds.md`）**均存在且非空**
  违反 ⇒ **告警**（**不判红**）。

【口径（四条，写清以免误用）】
  1. **范围＝工作区记忆目录**（★ **在仓库之外**）⇒ 路径解析**同 `check_source_of_truth.py`**：
     `repo/../../.workbuddy/memory/`（回退 `repo/../.workbuddy/memory/`）。
  2. ★ **CI 降级（`--ci`）**：CI 上**工作区记忆不在仓库内、必然不存在** ⇒ **只 warn、不告警**
     （**同源先例**＝`check_source_of_truth.py` 判据 B；依据 **R14「本机绿是假绿」**）
     ⇒ **本判据的效力范围＝本机**。★ 这是**如实的边界**，不是把缺陷藏起来。
  3. **上限数字与机制 30 同源**（**1024／4096**）⇒ **不在此另立口径**（**C19 同源**）。
  4. **内容层只验"存在且非空"**；**不验内容正确性**（内容正确性由 `T-046` 维护纪律 ＋ 人工守）。

【自检】
  `--selftest`：正反对照**十侧** ——
    ① 合规目录 ⇒ **无告警**（阳性对照）
    ② 调用层 **1024 B**（**边界值**）⇒ 必须告警（正例；`<` **不是** `<=`）
    ③ 调用层 **1023 B** ⇒ **不告警**（反例；**边界另一侧** —— 防"把上限写成 ≥"）
    ④ 索引层 **4096 B**（边界值）⇒ 必须告警（正例）
    ⑤ 索引层 **4095 B** ⇒ 不告警（反例）
    ⑥ 内容层**缺 1 份** ⇒ 必须告警（正例）
    ⑦ 内容层**空文件** ⇒ 必须告警（正例）
    ⑧ ★ **目录不存在 ＋ `--ci`** ⇒ **只 warn、不告警**（反例；**CI 场景对照**）
    ⑨ ★ **目录不存在 ＋ 非 CI** ⇒ **必须告警**（正例；★ **"不存在"与"合规"必须可区分** —— **R83／R89**）
    ⑩ **回读真实本机目录**（**C18**）：三文件**实际读到且字节 > 0**，并**打印实测值**（**不静默**）

【用法】
  check_memory_layers.py              # 实跑（**只告警**；退出码恒 0）
  check_memory_layers.py --ci          # CI 模式（工作区记忆缺失 ⇒ 只 warn）
  check_memory_layers.py --selftest    # 自检（十侧正反对照）

【触发再评估】
  告警命中**连续 N=5 轮为 0** ⇒ 再议转判红（与 `check_id_set_diff.py`／`check_report_structure.py` 同一口径）。
"""
from __future__ import annotations

import argparse
import io
import os
import sys
import tempfile

# ★ 上限与机制 30（`T-046`）**同源**：调用层 < 1 KB、索引层 < 4 KB
LIMITS = [
    ("调用层", "MEMORY.md", 1024, "< 1 KB"),
    ("索引层", "MEMORY_INDEX.md", 4096, "< 4 KB"),
]
CONTENT = ["ironlaws.md", "env.md", "state.md", "rounds.md"]

TOOL_REL = "coordination/tools/check_memory_layers.py"


def repo_root() -> str:
    here = os.path.dirname(os.path.abspath(__file__))
    return os.path.abspath(os.path.join(here, "..", ".."))


def resolve_memdir(repo: str, override: str | None = None) -> str:
    """工作区记忆目录（★ 在仓库之外）。解析口径与 `check_source_of_truth.py` 一致。"""
    if override:
        return os.path.abspath(override)
    cands = [
        os.path.abspath(os.path.join(repo, "..", "..", ".workbuddy", "memory")),
        os.path.abspath(os.path.join(repo, "..", ".workbuddy", "memory")),
    ]
    for c in cands:
        if os.path.isdir(c):
            return c
    return cands[0]


def evaluate(memdir: str, ci: bool = False):
    """返回 (alerts, warns, rows)；rows＝[(层, 文件, 实测字节|None, 上限|None, ok)]"""
    alerts, warns, rows = [], [], []
    if not os.path.isdir(memdir):
        msg = "工作区记忆目录不存在：%s" % memdir
        (warns if ci else alerts).append(msg)
        return alerts, warns, rows

    for layer, fn, limit, label in LIMITS:
        p = os.path.join(memdir, fn)
        if not os.path.exists(p):
            rows.append((layer, fn, None, limit, False))
            alerts.append("%s 缺失：`%s`" % (layer, fn))
            continue
        b = os.path.getsize(p)
        ok = b < limit
        rows.append((layer, fn, b, limit, ok))
        if not ok:
            alerts.append("%s 超限：`%s` = **%d B** ≥ %d（%s）" % (layer, fn, b, limit, label))

    for fn in CONTENT:
        p = os.path.join(memdir, fn)
        if not os.path.exists(p):
            rows.append(("内容层", fn, None, None, False))
            alerts.append("内容层 缺失：`%s`" % fn)
            continue
        b = os.path.getsize(p)
        ok = b > 0
        rows.append(("内容层", fn, b, None, ok))
        if not ok:
            alerts.append("内容层 空文件：`%s`" % fn)

    return alerts, warns, rows


def _pad(n: int) -> str:
    """生成恰好 n 字节的 ASCII 文本。"""
    return "x" * n


def _make(d, call=None, index=None, content=None):
    """造夹具目录并**回写**。

    ★ **参数语义（写清，防埋坑）**：`None` **＝「用合规默认值」**（**不是**"缺文件"）。
      —— 要造"缺文件／空文件"的场景，**必须显式 `os.remove()` / 写空串**（见自检侧⑥⑦）。
      （初版曾写 `if call is not None:` 再判一次 ⇒ **死分支 ＋ 语义误导**，2026-09-22 当场修掉。）
    """
    os.makedirs(d, exist_ok=True)
    call = 977 if call is None else call
    index = 4020 if index is None else index
    content = 200 if content is None else content
    io.open(os.path.join(d, "MEMORY.md"), "w", encoding="utf-8", newline="\n").write(_pad(call))
    io.open(os.path.join(d, "MEMORY_INDEX.md"), "w", encoding="utf-8", newline="\n").write(_pad(index))
    for fn in CONTENT:
        io.open(os.path.join(d, fn), "w", encoding="utf-8", newline="\n").write(_pad(content))


def selftest():
    fails = []
    with tempfile.TemporaryDirectory() as td:
        d = os.path.join(td, "memory")

        # 侧① 合规目录 ⇒ 无告警（阳性对照）
        _make(d)
        e, w, rows = evaluate(d)
        ok = (not e) and (not w) and len(rows) == len(LIMITS) + len(CONTENT)
        print("  侧①（阳性·合规目录 ⇒ 无告警）: %s  告警=%d 行=%d" % ("PASS" if ok else "FAIL", len(e), len(rows)))
        if not ok:
            fails.append("侧①")

        # 侧② 调用层 1024 B（边界值）⇒ 必须告警
        _make(d, call=1024)
        e, _, _ = evaluate(d)
        ok = any("调用层" in x and "超限" in x for x in e)
        print("  侧②（正例·调用层 1024 B 边界 ⇒ 告警）: %s" % ("PASS" if ok else "FAIL"))
        if not ok:
            fails.append("侧②")

        # 侧③ 调用层 1023 B ⇒ 不告警（边界另一侧）
        _make(d, call=1023)
        e, _, _ = evaluate(d)
        ok = not any("调用层" in x for x in e)
        print("  侧③（反例·调用层 1023 B ⇒ 不告警）: %s" % ("PASS" if ok else "FAIL"))
        if not ok:
            fails.append("侧③")

        # 侧④ 索引层 4096 B（边界值）⇒ 必须告警
        _make(d, index=4096)
        e, _, _ = evaluate(d)
        ok = any("索引层" in x and "超限" in x for x in e)
        print("  侧④（正例·索引层 4096 B 边界 ⇒ 告警）: %s" % ("PASS" if ok else "FAIL"))
        if not ok:
            fails.append("侧④")

        # 侧⑤ 索引层 4095 B ⇒ 不告警
        _make(d, index=4095)
        e, _, _ = evaluate(d)
        ok = not any("索引层" in x for x in e)
        print("  侧⑤（反例·索引层 4095 B ⇒ 不告警）: %s" % ("PASS" if ok else "FAIL"))
        if not ok:
            fails.append("侧⑤")

        # 侧⑥ 内容层缺 1 份 ⇒ 必须告警
        _make(d)
        os.remove(os.path.join(d, "rounds.md"))
        e, _, _ = evaluate(d)
        ok = any("内容层 缺失" in x and "rounds.md" in x for x in e)
        print("  侧⑥（正例·内容层缺 rounds.md ⇒ 告警）: %s" % ("PASS" if ok else "FAIL"))
        if not ok:
            fails.append("侧⑥")

        # 侧⑦ 内容层空文件 ⇒ 必须告警
        _make(d)
        io.open(os.path.join(d, "env.md"), "w", encoding="utf-8").write("")
        e, _, _ = evaluate(d)
        ok = any("内容层 空文件" in x and "env.md" in x for x in e)
        print("  侧⑦（正例·内容层空 env.md ⇒ 告警）: %s" % ("PASS" if ok else "FAIL"))
        if not ok:
            fails.append("侧⑦")

        # 侧⑧ 目录不存在 ＋ --ci ⇒ 只 warn、不告警（CI 场景对照）
        e, w, _ = evaluate(os.path.join(td, "NO_SUCH_DIR"), ci=True)
        ok = (not e) and bool(w)
        print("  侧⑧（反例·目录不存在 ＋ --ci ⇒ 只 warn）: %s" % ("PASS" if ok else "FAIL"))
        if not ok:
            fails.append("侧⑧")

        # 侧⑨ 目录不存在 ＋ 非 CI ⇒ 必须告警（"不存在"与"合规"可区分）
        e, w, _ = evaluate(os.path.join(td, "NO_SUCH_DIR"), ci=False)
        ok = bool(e) and (not w)
        print("  侧⑨（正例·目录不存在 ＋ 非 CI ⇒ 告警）: %s" % ("PASS" if ok else "FAIL"))
        if not ok:
            fails.append("侧⑨")

        # 侧⑩ 回读真实本机目录（C18）
        real = resolve_memdir(repo_root())
        e, w, rows = evaluate(real)
        got = [r for r in rows if r[2] is not None and r[2] > 0]
        ok = (len(got) == len(LIMITS) + len(CONTENT))
        print("  侧⑩（对照·回读真实目录 %s：读到 %d/%d 份）: %s"
              % (os.path.basename(os.path.dirname(real)), len(got), len(LIMITS) + len(CONTENT),
                 "PASS" if ok else "FAIL"))
        if not ok:
            fails.append("侧⑩")

    print("=" * 62)
    if fails:
        print("自检结论：FAIL %d 项 %s" % (len(fails), fails))
        return 1
    print("自检结论：PASS（十侧：5 正例 ＋ 3 反例 ＋ 2 对照）")
    return 0


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--repo", default=repo_root())
    ap.add_argument("--memory-dir", default=None)
    ap.add_argument("--ci", action="store_true", help="CI 模式：工作区记忆缺失只 warn")
    ap.add_argument("--selftest", action="store_true")
    a = ap.parse_args()

    if a.selftest:
        return selftest()

    memdir = resolve_memdir(a.repo, a.memory_dir)
    print("=" * 66)
    print("MEMORY 三层指针机制判据（机制 30 ／ `T-046`）★ 先只告警，不判红")
    print("工作区记忆目录：%s" % memdir)
    print("=" * 66)

    alerts, warns, rows = evaluate(memdir, ci=a.ci)
    for w in warns:
        print("  WARN  %s" % w)
    print("\n%-6s %-18s %10s %10s   %s" % ("层", "文件", "实测(B)", "上限(B)", "结论"))
    for layer, fn, b, limit, ok in rows:
        print("%-6s %-18s %10s %10s   %s"
              % (layer, fn,
                 ("%d" % b) if b is not None else "—",
                 ("%d" % limit) if limit is not None else "—",
                 "OK" if ok else "★ 违反"))

    if a.ci and not rows:
        print("\n★ CI 模式：工作区记忆目录不在仓库内（CI 上必然不存在）⇒ **只 warn、不判红**。")
        print("   效力范围＝**本机**（同源先例：`check_source_of_truth.py` 判据 B；依据 **R14**）。")

    print("\n" + "-" * 66)
    if alerts:
        print("⚠️ 告警 %d 条：" % len(alerts))
        for x in alerts:
            print("   - %s" % x)
    else:
        print("✅ 无告警（三层均达标：调用层 < 1024 B、索引层 < 4096 B、内容层 4 份存在且非空）")

    print("\n" + "=" * 66)
    print("告警合计：%d" % len(alerts))
    print("★ 本判据**先只告警、不判红**（退出码恒 0）；触发再评估＝告警命中**连续 N=5 轮为 0**。")
    print("=" * 66)
    return 0


if __name__ == "__main__":
    sys.exit(main())
