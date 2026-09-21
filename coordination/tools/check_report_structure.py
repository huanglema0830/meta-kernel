#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""报告结构判据（★ 先只告警、不判红 · 2026-09-21 · v0.309 轮立项）

【为什么】
  `TEMPLATES.md §〇 固定相 B` 规定**报告必带十四项**（第 0 项＝顾问阅读指令）。
  但截至 v0.308，**十四项中只有第 0 项有机器判据**（`check_reading_instruction.py`），
  其余 13 项**全靠人守** ⇒ 缺项无人拦（`BASELINE §§四十七–四十八` 登记的 **F-3**）。

【一句话判据】
  `coordination/reports/*.md` 中，**处于「报告体」形态**且**不在冻结豁免名单**内的文件，
  **必须**含 `TEMPLATES.md §〇 固定相 B` 的**十四项**；缺项 ⇒ **告警**。

【口径（四条，写清以免误用）】
  1. **范围**＝`coordination/reports/*.md`（与 `check_reading_instruction.py` 同范围，便于对照）。
  2. **"报告体"判定**＝该文件**含「启动回执」**。理由：十四项里只有「启动回执」是**轮次报告独有**的形态标记；
     `reports/` 下还混有**评估稿／核对稿／清单／讨论稿**（**不是**十四项报告）⇒ 不判定就会大面积假告警。
     ⇒ **非报告体**文件**不参与检查**，但**打印计数**（**不静默**）。
  3. **第 0 项＝复用 `check_reading_instruction.py` 的解析**（**import 复用，不另写第二份** —— **C19 同源**）。
  4. **冻结豁免（丁案 · 不回填）**：既有报告**不回填**（改历史快照＝篡改留痕）；其**实名清单**在
     `coordination/security/report_structure_exempt.txt`，**只减不增**（新增条目＝判红）。

【检查项（**十四项**；同义词表**打印可见** —— 防"关键词命中即通过"）】
  0 顾问阅读指令（**复用 C13 判据**，要求在第 0 行）
  1 保密标记      2 全景状态卡   3 任务＋基准＋定位   4 状态
  5 验收对照      6 未完成项     7 真实缺陷           8 三量台账（**四量齐**）
  9 边界说明（**四类齐**）        10 决策评估（七要素） 11 需要用户决策
  12 下一步建议   13 启动回执

【自检】
  `--selftest`：正反对照**十侧** ——
    ① 十四项齐备 ⇒ 无缺失（反例）
    ② 抽掉第 8 项（三量台账）⇒ 必须报缺（正例）
    ③ 无「启动回执」⇒ 判**非报告体**、**跳过**（反例；防"评估稿被当报告"）
    ④ 三量台账**四量缺一**（去"创新增量"）⇒ 必须报缺（反例；防"关键词命中即通过"）
    ⑤ 边界说明**四类缺一**（去"未验证"）⇒ 必须报缺（反例）
    ⑥ 同义词：「待裁」代「需要用户决策」⇒ 不缺第 11 项（反例）
    ⑦ 同义词：「未完成」代「未完成项」⇒ 不缺第 6 项（反例）
    ⑧ 检查项数 **== 14**（防漏项；与固定相 B 同源声明）
    ⑨ 豁免名单**只减不增**（条目 ≤ 冻结数）且**每条仍存在**（C15 精神）
    ⑩ **回读真实仓库**（C18）：`glob` 到报告 **>0** 份，且**报告体 >0** 份（"0"与"解析失败"可区分）

【用法】
  check_report_structure.py              # 实跑（只告警；退出码恒 0）
  check_report_structure.py --selftest    # 自检（十侧正反对照）
  check_report_structure.py --freeze      # 按当前实测重写豁免名单（＝冻结基线；须显式调用）

【触发再评估】
  告警命中**连续 N=5 轮为 0** ⇒ 再议转判红（与 `check_id_set_diff.py` 同一口径）。
"""
from __future__ import annotations

import argparse
import glob
import io
import os
import re
import sys

REPORTS_GLOB = "coordination/reports/*.md"
EXEMPT_REL = "coordination/security/report_structure_exempt.txt"
FROZEN_MARK = "# 冻结条目数:"

# ★ 报告体判定的唯一标记（十四项里只有它是"轮次报告"独有）
BODY_MARK = "启动回执"

# 十四项：编号 → (名称, 判定函数, 同义词说明)
CHECKS = [
    ("0", "顾问阅读指令", lambda s: _read_directive_ok(s),
     "**复用 `check_reading_instruction.py`**（C19 同源）：字面第 0 行起于固定文本"),
    ("1", "保密标记", lambda s: bool(re.search(r"保密标记|保密级别", s)), "保密标记／保密级别"),
    ("2", "全景状态卡", lambda s: bool(re.search(r"全景状态卡|状态卡", s)), "全景状态卡／状态卡"),
    ("3", "任务（含基准/定位）", lambda s: "任务" in s, "任务"),
    ("4", "状态", lambda s: bool(re.search(r"状态[^\n]{0,12}(完成|部分完成|未完成)", s)),
     "状态＋(完成/部分完成/未完成)"),
    ("5", "验收对照", lambda s: bool(re.search(r"验收对照|验收标准", s)), "验收对照／验收标准"),
    ("6", "未完成项", lambda s: "未完成" in s, "未完成项／未完成"),
    ("7", "真实缺陷", lambda s: bool(re.search(r"真实缺陷|缺陷", s)), "真实缺陷／缺陷"),
    ("8", "三量台账（**四量**）", lambda s: all(k in s for k in ("存量", "变量", "补充增量", "创新增量")),
     "存量＋变量＋补充增量＋创新增量（**四量齐**）"),
    ("9", "边界说明（**四类**）", lambda s: all(k in s for k in ("本地跑过", "CI过", "真实跑过", "未验证")),
     "本地跑过＋CI过＋真实跑过＋未验证（**四类齐**）"),
    ("10", "决策评估", lambda s: bool(re.search(r"决策评估|七要素", s)), "决策评估／七要素"),
    ("11", "需要用户决策", lambda s: bool(re.search(r"需要用户决策|需要你决策|待裁|待裁定|需要决策", s)),
     "需要用户决策／需要你决策／待裁／待裁定／需要决策"),
    ("12", "下一步建议", lambda s: bool(re.search(r"下一步建议|下一步", s)), "下一步建议／下一步"),
    ("13", "启动回执", lambda s: BODY_MARK in s, "启动回执"),
]
N_ITEMS = 14


def _read_directive_ok(s):
    """★ 第 0 项：**复用** C13 判据的解析路径（C19 同源），不另写第二份。"""
    try:
        import importlib.util
        here = os.path.dirname(os.path.abspath(__file__))
        spec = importlib.util.spec_from_file_location(
            "cri", os.path.join(here, "check_reading_instruction.py"))
        mod = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(mod)
        if not s.lstrip().startswith(mod.START):
            return False
        return mod.block_of(s) not in (None, "TRUNCATED")
    except Exception:                                        # noqa: BLE001
        return False


def repo_root() -> str:
    here = os.path.dirname(os.path.abspath(__file__))
    return os.path.abspath(os.path.join(here, "..", ".."))


def read_text(path: str) -> str:
    return io.open(path, encoding="utf-8", errors="replace").read()


def is_body(text: str) -> bool:
    return BODY_MARK in text


def missing_items(text: str):
    return [("%s %s" % (n, name)) for n, name, fn, _ in CHECKS if not fn(text)]


def load_exempt(path: str):
    """返回 (名单 list[str], 冻结条目数 int|None)。"""
    if not os.path.exists(path):
        return [], None
    names, frozen = [], None
    for line in read_text(path).splitlines():
        t = line.strip()
        if t.startswith(FROZEN_MARK):
            try:
                frozen = int(t.split(":", 1)[1].strip())
            except ValueError:
                frozen = None
        elif t and not t.startswith("#"):
            names.append(t)
    return names, frozen


def scan(repo: str):
    """返回 (报告总数, 报告体数, 明细 list[(rel, missing, exempt?)], 非报告体数)"""
    paths = sorted(glob.glob(os.path.join(repo, REPORTS_GLOB)))
    total = len(paths)
    bodies, non_bodies, rows = 0, 0, []
    for p in paths:
        s = read_text(p)
        rel = os.path.basename(p)
        if not is_body(s):
            non_bodies += 1
            continue
        bodies += 1
        rows.append((rel, missing_items(s)))
    return total, bodies, rows, non_bodies


def selftest():
    fails = []

    ok_full = (
        "【顾问阅读指令 · 请先执行】\n1. 全面阅读\n如未执行以上步骤，视为阅读不到位。\n\n"
        "**① 保密标记** **② 全景状态卡** **③ 任务** **④ 状态：完成**\n"
        "**⑤ 验收对照** **⑥ 未完成项** **⑦ 真实缺陷**\n"
        "**⑧ 三量台账**：存量／变量／补充增量／创新增量\n"
        "**⑨ 边界说明**：本地跑过／CI过／真实跑过／未验证\n"
        "**⑩ 决策评估** **⑪ 需要用户决策** **⑫ 下一步建议**\n**⑬ 启动回执**\n"
    )

    # 侧① 十四项齐备 ⇒ 无缺失
    bad = missing_items(ok_full)
    ok = (len(bad) == 0)
    print("  侧①（反例·十四项齐备 ⇒ 无缺失）: %s  缺=%s" % ("PASS" if ok else "FAIL", bad))
    if not ok:
        fails.append("侧①")

    # 侧② 抽掉第 8 项（三量台账）⇒ 必须报缺
    bad = missing_items(ok_full.replace("**⑧ 三量台账**：存量／变量／补充增量／创新增量\n", ""))
    ok = any(b.startswith("8 ") for b in bad)
    print("  侧②（正例·抽掉三量台账 ⇒ 报缺 8）: %s  缺=%s" % ("PASS" if ok else "FAIL", bad))
    if not ok:
        fails.append("侧②")

    # 侧③ 无「启动回执」⇒ 判非报告体、跳过
    no_body = ok_full.replace("**⑬ 启动回执**", "")
    ok = (not is_body(no_body))
    print("  侧③（反例·无启动回执 ⇒ 非报告体、跳过）: %s" % ("PASS" if ok else "FAIL"))
    if not ok:
        fails.append("侧③")

    # 侧④ 三量台账**四量缺一** ⇒ 必须报缺（防"关键词命中即通过"）
    bad = missing_items(ok_full.replace("创新增量", "X"))
    ok = any(b.startswith("8 ") for b in bad)
    print("  侧④（反例·四量缺一 ⇒ 报缺 8）: %s" % ("PASS" if ok else "FAIL"))
    if not ok:
        fails.append("侧④")

    # 侧⑤ 边界说明**四类缺一** ⇒ 必须报缺
    bad = missing_items(ok_full.replace("未验证", "X"))
    ok = any(b.startswith("9 ") for b in bad)
    print("  侧⑤（反例·四类缺一 ⇒ 报缺 9）: %s" % ("PASS" if ok else "FAIL"))
    if not ok:
        fails.append("侧⑤")

    # 侧⑥ 同义词：「待裁」代「需要用户决策」⇒ 不缺第 11 项
    bad = missing_items(ok_full.replace("**⑪ 需要用户决策**", "**⑪ 本轮待裁**"))
    ok = not any(b.startswith("11 ") for b in bad)
    print("  侧⑥（反例·同义词「待裁」⇒ 不缺 11）: %s" % ("PASS" if ok else "FAIL"))
    if not ok:
        fails.append("侧⑥")

    # 侧⑦ 同义词：「未完成」代「未完成项」⇒ 不缺第 6 项
    bad = missing_items(ok_full.replace("**⑥ 未完成项**", "**⑥ 未完成**"))
    ok = not any(b.startswith("6 ") for b in bad)
    print("  侧⑦（反例·同义词「未完成」⇒ 不缺 6）: %s" % ("PASS" if ok else "FAIL"))
    if not ok:
        fails.append("侧⑦")

    # 侧⑧ 检查项数 == 14（防漏项）
    ok = (len(CHECKS) == N_ITEMS)
    print("  侧⑧（对照·检查项数 == %d）: %s  实测=%d" % (N_ITEMS, "PASS" if ok else "FAIL", len(CHECKS)))
    if not ok:
        fails.append("侧⑧")

    # 侧⑨ 豁免名单只减不增 ＋ 每条仍存在
    repo = repo_root()
    names, frozen = load_exempt(os.path.join(repo, EXEMPT_REL))
    miss = [n for n in names if not os.path.exists(os.path.join(repo, "coordination/reports", n))]
    ok = (frozen is not None) and (len(names) <= frozen) and (not miss)
    print("  侧⑨（对照·豁免名单只减不增且条目存在）: %s  条目=%s 冻结=%s 缺失=%s" % (
        "PASS" if ok else "FAIL", len(names), frozen, miss[:3]))
    if not ok:
        fails.append("侧⑨")

    # 侧⑩ 回读真实仓库（C18）：报告 >0 且 报告体 >0
    total, bodies, _, _ = scan(repo)
    ok = (total > 0) and (bodies > 0)
    print("  侧⑩（对照·回读真实仓库：报告 %d／报告体 %d）: %s" % (total, bodies, "PASS" if ok else "FAIL"))
    if not ok:
        fails.append("侧⑩")

    print("=" * 62)
    if fails:
        print("自检结论：FAIL %d 项 %s" % (len(fails), fails))
        return 1
    print("自检结论：PASS（十侧：4 正例 + 4 同义/结构反例 + 2 对照）")
    return 0


def do_freeze():
    repo = repo_root()
    total, bodies, rows, non_bodies = scan(repo)
    names = sorted([rel for rel, miss in rows if miss])
    path = os.path.join(repo, EXEMPT_REL)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with io.open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write("# 报告结构判据 · 冻结豁免名单（**只减不增**）\n")
        f.write("# 冻结条目数: %d\n" % len(names))
        f.write("#\n")
        f.write("# 口径（丁案）：既有报告**不回填**（改历史快照＝篡改留痕）。\n")
        f.write("#   本名单＝**冻结时点**已存在的、缺项的**报告体**文件（实名）。\n")
        f.write("#   ★ **只减不增**：新增条目＝判红（`--selftest` 侧⑨ 检查）。\n")
        f.write("#   ★ 非报告体（评估稿／核对稿／清单／讨论稿）**不入名单** —— 它们在检查范围外。\n")
        f.write("# 判据：`coordination/tools/check_report_structure.py`｜规则：`TEMPLATES.md §〇 固定相 B`\n")
        f.write("# 冻结时点：2026-09-21（v0.309 轮立项）｜当时实测：报告 %d 份／报告体 %d 份／非报告体 %d 份\n"
                % (total, bodies, non_bodies))
        for n in names:
            f.write(n + "\n")
    print("已写 %s（%d 条）" % (EXEMPT_REL, len(names)))
    return 0


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--selftest", action="store_true", help="自检（十侧正反对照）")
    ap.add_argument("--freeze", action="store_true", help="按当前实测重写豁免名单（＝冻结基线）")
    a = ap.parse_args()
    if a.selftest:
        return selftest()
    if a.freeze:
        return do_freeze()

    repo = repo_root()
    print("=" * 66)
    print("报告结构判据（★ 先只告警，不判红）   仓库根：%s" % os.path.basename(repo))
    print("=" * 66)

    total, bodies, rows, non_bodies = scan(repo)
    print("扫描 %d 份报告 ｜ **报告体** %d ｜ 非报告体 %d（**不参与检查**，如实计数 —— 不静默）"
          % (total, bodies, non_bodies))
    print("报告体判定：含「%s」（十四项里唯一「轮次报告」独有标记）" % BODY_MARK)
    print("检查项＝**%d 项**；同义词表：" % len(CHECKS))
    for n, name, _, syn in CHECKS:
        print("    %-3s %-14s ← %s" % (n, name, syn))

    names, frozen = load_exempt(os.path.join(repo, EXEMPT_REL))
    print("\n冻结豁免：%d 条（冻结条目数 %s）｜文件＝`%s`" % (len(names), frozen, EXEMPT_REL))

    alerts, exempt_hit = [], 0
    for rel, miss in rows:
        if rel in names:
            exempt_hit += 1
            continue
        if miss:
            alerts.append((rel, miss))

    if exempt_hit != len(names):
        print("\n⚠️ 豁免名单有 %d 条**未命中**（文件改名/删除？）—— **不静默**" % (len(names) - exempt_hit))

    print("\n" + "-" * 66)
    if alerts:
        print("⚠️ 告警 %d 份（缺项 ⇒ 报告结构不完整）：" % len(alerts))
        for rel, miss in alerts:
            print("   - %s" % rel)
            print("       缺：%s" % "、".join(miss))
    else:
        print("✅ 无告警（报告体非豁免者，十四项齐备）")

    print("\n" + "=" * 66)
    print("告警合计：%d" % len(alerts))
    print("★ 本判据**先只告警、不判红**（退出码恒 0）；触发再评估＝告警命中**连续 N=5 轮为 0**。")
    print("=" * 66)
    return 0


if __name__ == "__main__":
    sys.exit(main())
