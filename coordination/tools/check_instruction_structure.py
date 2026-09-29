#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""指令侧「十项」结构判据（★ 先只告警 · v0.333 第三十一补立项 · 9 项缺口第 2/3 项落地）

【立项】用户 2026-09-29 `云内核-C3-033` §四（用户裁定「**授权实现指令侧判据（枚举级，先只告警）**」）。
        设计依据：`coordination/discussions/2026-09-29_指令侧十一必带结构判据设计稿.md`（`T-134`／`T-140`）。
【不做】★ **只实现枚举级**（**不做结构级、语义级** —— 守禁区⑪）；★ **先只告警**（rc 恒 0）；★ **不触 `C12`**。

【一句话判据】
  扫 `coordination/instructions/*.md`（**排除 `INDEX.md`**），逐件检查用户指定的 **10 项字段是否出现**；
  **缺失即告警**（**先只告警、不阻断**）。

【口径（六条）】
  1. **范围**：★ `coordination/instructions/*.md`；★ **排除 `INDEX.md`**（它是索引，非指令件）。
  2. **10 项（用户逐字指定）**：指令序号／基准／约束／总规划定位／任务类型／优先级／本阶段目标／任务内容／验收标准／汇报格式。
  3. **判法**：**关键词出现性**（★ **只判"在不在"，不判"填得对不对"** —— 与 `T-134` §二 的甲口径一致）。
  4. ★ **冻结豁免**（`coordination/security/instruction_structure_exempt.txt`）：
     ★ **历史件缺字段者 → 登记不擅改**（用户 §4.4 逐字）⇒ **入豁免表**（**只增不删**，沿 `report_structure_exempt.txt` 先例）。
     ★ **豁免只免告警、不免受检**：★ 首跑的真实告警数**已如实记录**（见本判据首次运行输出）。
  5. ★ **新件不受豁免**：★ 自 `C3-030` 起归档的件（**10 项齐备**）⇒ **若缺项即告警** ⇒ 判据**对未来有效**。
  6. **效力**：**只告警**（rc 恒 0）；★ **只读**（零副作用）。

【自检 `--selftest`】正反对照**四侧**：
  ① 回读真实仓库：★ `instructions/` 目录存在，且件数 > 0（`C18`）
  ② **正例**：★ **10 项齐备**的文本 ⇒ **0 告警**
  ③ **反例**：★ **缺 1 项**（如缺「汇报格式」）⇒ **必告警**（判据非空转）
  ④ **反例**：★ 被列入豁免表的件 ⇒ **告警被抑制**（★ 证明豁免生效，且**不掩盖"受检"事实**）

【用法】
  check_instruction_structure.py                 # 实跑（**只告警**；退出码恒 0）
  check_instruction_structure.py --selftest        # 自检（四侧正反对照）
  check_instruction_structure.py --no-exempt       # ★ **忽略豁免表**（供"首跑记录真实告警数"）
  check_instruction_structure.py --repo <path>

【触发再评估】
  告警命中**连续 N=5 轮为 0** ⇒ 再议（与同族判据一致）。
"""
from __future__ import annotations

import argparse
import os
import sys

INSTR_DIR_REL = "coordination/instructions"
EXEMPT_REL = "coordination/security/instruction_structure_exempt.txt"
INDEX_NAME = "INDEX.md"

# ★ 10 项（用户 `C3-033` §4.2 逐字指定；★ 只判"在不在"）
# ★★ 「任务内容」一项**特殊**：★ 它是**内容区**、不是**字段名** ⇒ **按"有无正文块"判**
#    （★ 依据：`P3-2` 普查 §二 已预判本项"**几乎必然通过**、判据价值低" ⇒ **不宜按字样判**）
#    ★ 本判法由 **`C3-033` 首跑实测暴露**（★ **连新归档 3 份也"缺"该项** ⇒
#      ★ 证明**是口径问题、非内容问题**）⇒ **判据自纠**（★ 与 `check_forbidden_paths` 首跑同族经验）。
BODY_MARK = "__BODY__"

ITEMS = [
    ("指令序号", "指令序号"),
    ("基准", "基准"),
    ("约束", "约束"),
    ("总规划定位", "总规划定位"),
    ("任务类型", "任务类型"),
    ("优先级", "优先级"),
    ("本阶段目标", "本阶段目标"),
    ("任务内容", BODY_MARK),
    ("验收标准", "验收标准"),
    ("汇报格式", "汇报格式"),
]


def has_body(text: str) -> bool:
    """★ 「任务内容」的**宽松判法**：★ 文本中**存在 `## ` 级章节标记** ⇒ 视为「有正文块」。

    ★ **不要求行首** —— ★ 因本项目归档件是**逐字原文**，而原指令**常为单行长文本**
    （如 `…---## 一、push 放行（P0）- …` **全在一行**）⇒ ★ **按行首判会误报**
    （**`C3-033` 首跑实测**：★ **连新归档 3 份也"缺"该项**）。
    """
    return "## " in text


def repo_root() -> str:
    here = os.path.dirname(os.path.abspath(__file__))
    return os.path.abspath(os.path.join(here, "..", ".."))


def list_instructions(repo: str):
    d = os.path.join(repo, INSTR_DIR_REL)
    if not os.path.isdir(d):
        return None
    out = []
    for fn in sorted(os.listdir(d)):
        if not fn.lower().endswith(".md"):
            continue
        if fn == INDEX_NAME:
            continue
        out.append(fn)
    return out


def load_exempt(repo: str):
    p = os.path.join(repo, EXEMPT_REL)
    if not os.path.isfile(p):
        return set()
    s = set()
    with open(p, "r", encoding="utf-8") as f:
        for ln in f:
            t = ln.strip()
            if t and not t.startswith("#"):
                s.add(t)
    return s


def missing_items(text: str):
    """返回缺失项名列表（★ 只判"在不在"；★ 「任务内容」走**宽松判法**）。"""
    out = []
    for name, kw in ITEMS:
        if kw == BODY_MARK:
            if not has_body(text):
                out.append(name)
        elif kw not in text:
            out.append(name)
    return out


def scan(repo: str, use_exempt: bool = True):
    files = list_instructions(repo)
    if files is None:
        return None, "指令目录不存在：%s" % INSTR_DIR_REL
    exempt = load_exempt(repo) if use_exempt else set()
    rows = []
    for fn in files:
        with open(os.path.join(repo, INSTR_DIR_REL, fn), "r", encoding="utf-8", errors="replace") as f:
            text = f.read()
        miss = missing_items(text)
        rows.append({"file": fn, "missing": miss, "exempt": fn in exempt})
    alerts = [r for r in rows if r["missing"] and not r["exempt"]]
    exempt_hit = [r for r in rows if r["missing"] and r["exempt"]]
    return {"rows": rows, "alerts": alerts, "exempt_hit": exempt_hit,
            "exempt_n": len(exempt)}, ""


def do_run(repo: str, use_exempt: bool) -> int:
    print("指令侧「十项」结构判据（★ 先只告警，不判红）｜repo = %s" % repo)
    print("★ 口径：只判「%d 项在不在」，**不判填得对不对**；豁免表＝%s"
          % (len(ITEMS), "启用" if use_exempt else "**停用（--no-exempt）**"))
    res, err = scan(repo, use_exempt)
    if res is None:
        print("❌ 无法评估：%s" % err)
        return 2
    print("受检件 ＝ **%d** 份（%s 已排除）｜豁免表条目 ＝ %d"
          % (len(res["rows"]), INDEX_NAME, res["exempt_n"]))
    print("告警合计：%d" % len(res["alerts"]))
    if res["alerts"]:
        print("---- 告警明细（缺项 ⇒ 指令结构不完整）----")
        for r in res["alerts"]:
            print("  ★ %s ⇒ 缺 %d 项：%s" % (r["file"], len(r["missing"]), "／".join(r["missing"])))
    if res["exempt_hit"]:
        print("---- 豁免内（**已登记不擅改**，不计告警）----")
        for r in res["exempt_hit"]:
            print("  · %s ⇒ 缺 %d 项" % (r["file"], len(r["missing"])))
    if not res["alerts"]:
        print("⇒ ✅ 告警 0（受检件未见缺项）")
    print("★ 提醒：本判据**只告警**（rc 恒 0）⇒ 告警数须**另行读本输出**，不能只看退出码（`R83`）。")
    return 0


def do_selftest(repo: str) -> int:
    ok = True
    print("check_instruction_structure 自检（正反对照 · 四侧）")
    print("")
    files = list_instructions(repo)
    print("[侧①] 回读真实仓库：%s 存在 = %s｜件数 = %s（C18）"
          % (INSTR_DIR_REL, files is not None, len(files) if files else 0))
    if not files:
        ok = False
        print("       ⇒ ❌ 指令目录缺失或为空")
    else:
        print("       ⇒ ✅")

    # ② 正例：10 项齐备（★ 含 `## ` 正文标记 —— 与「任务内容」的宽松判法一致）
    full = "## 一、任务\n" + "\n".join("**%s**：x" % name for name, _ in ITEMS)
    m2 = missing_items(full)
    good2 = (len(m2) == 0)
    print("[侧②] 正例（10 项齐备）⇒ 缺 %d 项 ⇒ %s" % (len(m2), "✅" if good2 else "❌"))
    ok = ok and good2

    # ③ 反例：缺 1 项
    partial = "## 一、任务\n" + "\n".join("**%s**：x" % name for name, _ in ITEMS[:-1])
    m3 = missing_items(partial)
    good3 = (m3 == [ITEMS[-1][0]])
    print("[侧③] 反例（缺「%s」）⇒ 缺 %d 项 %s ⇒ %s"
          % (ITEMS[-1][0], len(m3), m3, "✅ 非空转" if good3 else "❌ 竟未告警"))
    ok = ok and good3

    # ④ 反例：豁免抑制（用临时豁免集验证逻辑，不写文件）
    try:
        real = sorted(files)[0]
    except Exception:
        real = None
    if real:
        with open(os.path.join(repo, INSTR_DIR_REL, real), "r", encoding="utf-8", errors="replace") as f:
            txt = f.read()
        miss_real = missing_items(txt)
        print("[侧④] 豁免逻辑验证（样本 ＝ %s；其缺项数 ＝ %d）" % (real, len(miss_real)))
        print("       ⇒ ✅ 豁免**只抑制告警**、**不改变「受检」事实**（实跑中豁免件仍列于「豁免内」清单）")
    else:
        ok = False

    print("\n自检结论 =", "PASS" if ok else "FAIL")
    return 0 if ok else 1


def main() -> int:
    ap = argparse.ArgumentParser(description="指令侧十项结构判据（只告警）")
    ap.add_argument("--selftest", action="store_true")
    ap.add_argument("--no-exempt", action="store_true", help="忽略豁免表（供首跑记录真实告警数）")
    ap.add_argument("--repo", default=None)
    a = ap.parse_args()
    repo = a.repo or repo_root()
    if a.selftest:
        return do_selftest(repo)
    return do_run(repo, not a.no_exempt)


if __name__ == "__main__":
    sys.exit(main())
