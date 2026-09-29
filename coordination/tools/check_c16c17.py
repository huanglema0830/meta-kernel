#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""C16／C17 断言证据判据（**枚举级** · ★ 先只告警、不判红 · v0.333 第三十四补立项 · T-161）

【为什么】
  用户 `C3-036` §二 授权实现 `C16`／`C17` 判据（甲案：**单判据合一支 · 枚举级 · 先只告警 · 先窄后宽**）。
  取证（`T-156`）已证：★ `tools/*.py` 中 **`C16`／`C17` 零命中** ⇒ **本项此处起有机器守**。

【一句话判据】
  扫 `coordination/reports/*.md` ＋ `coordination/BASELINE.md`：
    A **`C17` 管辖词命中**（**8 词**：虚假声明／自相矛盾／名实不符／形同虚设／挂羊头卖狗肉／从未／全库无／底图落后）
    B **`C16` 断言词命中**（**4 词**：不存在／全库无／从未／没有定义）
  二者**命中即告警**（**只数命中、不判上下文** —— **枚举级**）。

【★ 口径（六条，写清以免误用）】
  1. **枚举级、非结构级**：本判据**不判**「该命中是否真属对底图的诚信类指控」——
     ★ 依据 `T-156`：**词表 20+ 处命中、0 处真指控** ⇒ **上下文判定机器做不到**。
     ⇒ 本判据**只做"提示"**：命中处**需人工复核**（并按 `C16` 四要素／`C17` 三问补证）。
  2. **先只告警**：退出码**恒 0**（`--strict` 才可变）；**不入 CI 判红**。
  3. **扫描范围（先窄）**：只 `reports/` ＋ `BASELINE.md` —— ★ **不扫全库**（守禁区⑬）。
  4. **★ 剔除（复用 `T-041` 机制）**：读 `scan_exclude.json` 的**既有 6 层**；
     ★ 本判据**另加 2 层**（见 `scan_exclude_c16c17.json`）——
     `L7 风险/后果描述段`、`L8 条文引述段`。
     ★★ **为什么不直接扩 `scan_exclude.json`**：该文件**被 `check_id_set_diff` 共用** ⇒
     扩展会**改变既有判据的输入**、**有回归风险** ⇒ ★ **按"只新增不删除"另立配置**（`T-161` 登记）。
  5. **每次扫描打印「已剔除段数」**（`T-041` 要求：**剔除不是静默的**）。
  6. **不重算机器事实**（`C18`）：本脚本**只读文本**；`--selftest` 须**回读真实仓库**。

【与现有 18 支的关系】
  ★ **无重叠**（`T-156` 已逐支读 docstring 验证）；★ 同族仅**机制上**复用 `check_id_set_diff` 的 `T-041` 剔除。
"""

import io
import json
import os
import re
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))          # 仓库根
CONF_MAIN = os.path.join(HERE, "scan_exclude.json")     # 既有（6 层）
CONF_OWN = os.path.join(HERE, "scan_exclude_c16c17.json")  # 本判据自有（2 层）

SCAN_DIRS = ["coordination/reports"]
SCAN_FILES = ["coordination/BASELINE.md"]

# ★★ 强弱词分级（`C3-037` §四 · 用户裁定「组1 甲」）
#   ★ 依据：★ 首跑实测（`T-161`）—— 10 词命中 395 处，但**强弱不同质**
#     （★ 强指控词 25 处可人工复核；★ 日常词 285 处远超复核量）。
#   ★ 强指控词（**命中即告警**）＝ 5 个
STRONG_WORDS = ["虚假声明", "名实不符", "形同虚设", "底图落后", "挂羊头卖狗肉"]
#   ★ 日常词（**另册列出、不计告警**）＝ 5 个（含 `C16` 的 4 词）
WEAK_WORDS = ["不存在", "自相矛盾", "从未", "全库无", "没有定义"]
# ★ 词源（不增不减）：`C17` 8 词 ＝ 强 5 ＋ 弱 3（自相矛盾／从未／全库无）；
#   `C16` 4 词 ＝ 全弱（不存在／全库无／从未／没有定义）⇒ 合并去重后 **强 5 ＋ 弱 5 ＝ 10 词**。


def load_json(p):
    """读配置；失败返回 None（★ 不静默降级 —— 由调用方报告）。"""
    try:
        with io.open(p, encoding="utf-8") as f:
            return json.load(f)
    except Exception:
        return None


def layer_marks(conf):
    """从一份配置取「层」列表（含 marks / line_prefixes）。"""
    out = []
    if not isinstance(conf, dict):
        return out
    for lay in conf.get("layers", []):
        out.append({
            "id": lay.get("id"),
            "name": lay.get("name", ""),
            "marks": lay.get("marks", []),
            "line_prefixes": lay.get("line_prefixes"),
        })
    return out


def is_excluded(line, layers):
    """★ 该行是否落在任一剔除层内。

    ★ 口径与 `T-041` 同源：层可自带 `line_prefixes` ⇒ **marks 只在「行以前缀开头」时生效**。
    ★ 命中弹出一层（**首层即返**，便于打印"已剔除段数"的分层统计）。
    """
    for lay in layers:
        pre = lay.get("line_prefixes")
        if pre:
            if not any(line.startswith(p) for p in pre):
                continue
        for m in lay.get("marks", []):
            if m and m in line:
                return lay
    return None


def scan_lines(lines, layers, hits_strong, hits_weak):
    """扫一组行；★ 与实跑**同一函数**（`C18`）。返回 (剔除行数, 强词命中, 弱词命中)。

    ★★ 强弱分册（`C3-037` §四）：强词 ⇒ **告警**；弱词 ⇒ **另册、不计告警**。
    ★ 一行可同时含强弱词 ⇒ 两册均记（**不去重**，便于按词统计）。
    """
    excluded = 0
    for i, raw in enumerate(lines, 1):
        s = raw.rstrip("\n")
        lay = is_excluded(s, layers)
        if lay:
            excluded += 1
            continue
        for w in STRONG_WORDS:
            if w in s:
                hits_strong.append((i, w))
        for w in WEAK_WORDS:
            if w in s:
                hits_weak.append((i, w))
    return excluded, hits_strong, hits_weak


def gather():
    """收集受检件（相对路径, 行列表）。"""
    files = []
    for d in SCAN_DIRS:
        full = os.path.join(REPO, d)
        if os.path.isdir(full):
            for n in sorted(os.listdir(full)):
                if n.endswith(".md"):
                    p = os.path.join(full, n)
                    files.append((os.path.join(d, n).replace(os.sep, "/"), p))
    for rel in SCAN_FILES:
        p = os.path.join(REPO, rel)
        if os.path.isfile(p):
            files.append((rel, p))
    return files


def scan_repo():
    """实跑真实仓库。返回 dict 或 None。"""
    conf_main = load_json(CONF_MAIN)
    conf_own = load_json(CONF_OWN)
    if conf_main is None or conf_own is None:
        return None
    layers = layer_marks(conf_main) + layer_marks(conf_own)

    files = gather()
    n_files = len(files)
    tot_excl = 0
    tot_strong = []
    tot_weak = []
    per_file = []
    for rel, p in files:
        try:
            with io.open(p, encoding="utf-8", errors="replace") as f:
                lines = f.readlines()
        except Exception:
            continue
        e, hs, hw = scan_lines(lines, layers, [], [])
        tot_excl += e
        for ln, w in hs:
            tot_strong.append((rel, ln, w))
        for ln, w in hw:
            tot_weak.append((rel, ln, w))
        if hs or hw:
            per_file.append((rel, len(hs), len(hw), e))
    return {
        "n_files": n_files,
        "excluded": tot_excl,
        "strong": tot_strong,
        "weak": tot_weak,
        "per_file": per_file,
        "n_layers": len(layers),
    }


def do_run(strict=False):
    res = scan_repo()
    if res is None:
        print("::error::配置读取失败（scan_exclude.json 或 scan_exclude_c16c17.json）")
        return 1
    print("受检件 ＝ **%d** 份（%s ＋ %s）｜剔除层 ＝ **%d** 层"
          % (res["n_files"], "／".join(SCAN_DIRS), "／".join(SCAN_FILES), res["n_layers"]))
    print("★ 已剔除段数（`T-041` 要求 · 剔除不是静默的）：**%d** 行" % res["excluded"])
    print("---- ★ 强指控词命中（**计告警 · 需人工复核**）----")
    for rel, ln, w in res["strong"]:
        print("  %s:%d  「%s」" % (rel, ln, w))
    print("---- ☆ 日常词命中（**另册 · 不计告警** · `C3-037` §四 组1 甲）----")
    for rel, ln, w in res["weak"]:
        print("  %s:%d  「%s」" % (rel, ln, w))
    n_al = len(res["strong"])
    print("告警合计：%d" % n_al)
    print("另册（日常词）合计：%d" % len(res["weak"]))
    print("★ 本判据**先只告警、不判红**（退出码恒 0）；★ 枚举级 —— **命中 ≠ 违规**"
          "（`T-156`：词表 20+ 处命中、0 处真指控）")
    if strict and n_al:
        return 1
    return 0


def do_selftest():
    """★ 正反对照（**含回读真实仓库** —— `C18`）。"""
    ok = True
    conf_main = load_json(CONF_MAIN)
    conf_own = load_json(CONF_OWN)
    if conf_main is None or conf_own is None:
        print("[侧①] ❌ 配置读取失败")
        return 1
    layers = layer_marks(conf_main) + layer_marks(conf_own)
    print("check_c16c17 自检（正反对照 · 六侧 · ★ 侧⑥ 回读真实仓库）")
    print("[侧①] 配置：既有 %d 层 ＋ 自有 %d 层 ⇒ 合计 %d 层 ⇒ %s"
          % (len(layer_marks(conf_main)), len(layer_marks(conf_own)), len(layers),
             "✅" if len(layers) >= 8 else "❌ 层数少于 8"))
    ok = ok and len(layers) >= 8

    # 侧② 正例：普通行无命中
    e, hs, hw = scan_lines(["这是一行普通说明，没有任何敏感词。"], layers, [], [])
    good = (not hs and not hw)
    print("[侧②] 正例（普通行）⇒ 强 %d／弱 %d ⇒ %s" % (len(hs), len(hw), "✅" if good else "❌"))
    ok = ok and good

    # 侧③ 反例：命中**强指控词** ⇒ 必告警
    e, hs, hw = scan_lines(["| **结论** | 该表述与条文**名实不符**。 |"], layers, [], [])
    good = (len(hs) == 1 and len(hw) == 0)
    print("[侧③] 反例（含强词「名实不符」· 非剔除层）⇒ 告警 %d ⇒ %s"
          % (len(hs), "✅ 判据非空转" if good else "❌ 未命中"))
    ok = ok and good

    # 侧④ 反例：命中**日常词** ⇒ 入另册、**不计告警**
    e, hs, hw = scan_lines(["经检索，该字段**不存在**。"], layers, [], [])
    good = (len(hs) == 0 and len(hw) == 1)
    print("[侧④] 反例（含日常词「不存在」）⇒ 告警 %d｜另册 %d ⇒ %s"
          % (len(hs), len(hw), "✅ 强弱分册生效" if good else "❌ 分册失效"))
    ok = ok and good

    # 侧⑤ 反例：**强词**落 L7（风险描述层）⇒ 不命中（剔除生效）
    e, hs, hw = scan_lines(["**风险**：若只做形式 ⇒ **形同虚设**。"], layers, [], [])
    good = (not hs and not hw and e == 1)
    print("[侧⑤] 反例（强词「形同虚设」但在 L7 风险层）⇒ 命中 %d｜剔除 %d ⇒ %s"
          % (len(hs) + len(hw), e, "✅ L7 生效" if good else "❌ 剔除边界失效"))
    ok = ok and good

    # 侧⑥ ★ 回读真实仓库
    res = scan_repo()
    if res is None:
        print("[侧⑥] ❌ 实跑失败")
        ok = False
    else:
        print("[侧⑥] ★ 回读真实仓库：受检 **%d** 份（**>0 期望**）｜剔除 **%d** 行｜"
              "★ 告警（强词）**%d**｜☆ 另册（弱词）**%d** ⇒ %s"
              % (res["n_files"], res["excluded"], len(res["strong"]), len(res["weak"]),
                 "✅ 真实仓库可解析" if res["n_files"] > 0 else "❌ 真实仓库零份"))
        ok = ok and (res["n_files"] > 0)

    print("自检结论：%s" % ("✅ 全 PASS" if ok else "❌ 有 FAIL"))
    return 0 if ok else 1


def main(argv):
    if "--selftest" in argv:
        return do_selftest()
    return do_run(strict=("--strict" in argv))


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
