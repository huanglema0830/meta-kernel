#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""底图回写五段流水线判据（`T-051` 的可执行载体 · ★ 先只告警、不判红 · v0.328 轮立项）

【为什么】
  `T-051`「底图回写五段流水线」已入条文（v0.327 轮），其规则逐字要求：
    「**① 出稿 → ② 待裁 → ③ `D16'` 解锁 → ④ 回写 → ⑤ 重新冻结**」＋
    「**③ 每段必留痕**」＋「**④ 单轮单次开闭**（一次解冻只做一批，做完即冻）」
  —— 但**无判据** ⇒ 与 **`F-5`（"有判定、无动作、无触发"）** 同族。本脚本即其落地件（B-2）。

【一句话判据】
  以台账 `coordination/BASELINE.md` 中**最后一个含「解冻」的节**为受检对象：
    **A** 五段标记须**可追溯**：段③④⑤ 须**在该节内**；
        段①②（出稿／待裁）**允许回看至多 3 个节**（★ 因为"出稿轮 → 待裁轮 → 回写轮"
        本就**可以跨轮**，`v0.325 只出稿` → `v0.326 回写` 即实例）。
    **B 半冻检测**：该节含「解冻」但**无**「重新冻结／重冻结」⇒ 告警（**单轮单次开闭**被破坏）。
  违反 ⇒ **告警**（**不判红**）。

【口径（五条）】
  1. **范围**：`coordination/BASELINE.md` 的 `## ` 顶层节。
  2. **受检对象**：「**最后一个含「解冻」或 `D16'` 的节**」——
     ★ 只对**做过回写／解除冻结**的节设要求；**纯出稿轮／纯裁定轮不设五段要求**（**边界写清**）。
  3. **不做的事**：**不判"每节都要五段"** —— 那会把 `T-051` 误读为"每轮五段"（**条文只说"回写一律分五段"**）。
  4. **不重算机器事实**（**C18**）：只读**文本标记**；故效力＝"**留痕是否齐**"，**不等于**"回写质量好坏"。
  5. ★★ **非回写轮豁免出口**（**v0.330 轮新增**）：口径 2 的"受检对象"是**近似判据** ——
     **"引述 `D16'`" ≠ "当轮走过回写程序"**。若受检节内出现 **`<!-- rewrite-pipeline-skip -->`**，
     则**该节当期不设五段要求**（**只出 note、不出告警**）。
     ★ **为什么必须有这个出口**：**纯出稿轮本无"解冻—回写—重冻"三件事** ⇒ 若不给出口，
     判据会**逼着人为了合判据而编留痕**（**那才是真风险**）；v0.330 轮实测该假告警为 **2 条**。

【自检】
  `--selftest`：正反对照**八侧** ——
    ① 五段齐全 ＋ 已重冻 ⇒ **无告警**（阳性对照）
    ② **缺段⑤（重冻）** ⇒ 告警（正例 · 半冻）
    ③ **缺段④（回写）** ⇒ 告警（正例）
    ④ **段①②在回看 3 节内** ⇒ 不告警（反例 · 跨轮追溯）
    ⑤ **段①②超出回看 3 节** ⇒ 告警（正例）
    ⑥ ★ **自述剔除**：含**本判据文件名**的行**不参与**（反例 · 治**自指污染第 12 例**）
    ⑦ ★ **非回写轮豁免出口**：受检节带 `<!-- rewrite-pipeline-skip -->` ⇒ **不告警**（反例 · 豁免出口）
    ⑧ **回读真实仓库**（**C18**）：真实台账**确实定位到受检节**，并**打印节标题与五段命中**

【用法】
  check_rewrite_pipeline.py              # 实跑（**只告警**；退出码恒 0）
  check_rewrite_pipeline.py --selftest    # 自检（八侧正反对照）
  check_rewrite_pipeline.py --repo <path>

【触发再评估】
  告警命中**连续 N=5 轮为 0** ⇒ 再议转判红（与 `check_id_set_diff.py` 同口径）。
"""
from __future__ import annotations

import argparse
import os
import sys

LOOKBACK = 3   # ★ 段①② 允许回看的节数（"出稿轮→待裁轮→回写轮"可跨轮）

# ★★ **非回写轮豁免出口**（`T-051` 边界的机器出口 · v0.330 轮新增）
#   为什么需要：`pick_target` 用「含『解冻』或 `D16'`」**近似**"做过回写的节"——
#   但**"引述 `D16'`"≠"当轮走过解冻程序"**。v0.330 轮实测：一个**纯出稿轮**
#   （只新增文件、**未改底图**）因**正文引述了 `D16'`** 而被误选为受检节 ⇒
#   **假告警 2 条**（缺段⑤ ＋ 半冻）。本出口让"确实非回写轮"的节**显式声明**，
#   以免判据逼着人**为合判据而编留痕**（那才是真风险）。
SKIP_MARK = "<!-- rewrite-pipeline-skip -->"

MARK = {
    "①出稿": ["出稿"],
    "②待裁": ["待裁"],
    "③解冻": ["解冻", "D16'"],
    "④回写": ["回写"],
    "⑤重冻": ["重新冻结", "重冻结"],
}


def repo_root() -> str:
    here = os.path.dirname(os.path.abspath(__file__))
    return os.path.abspath(os.path.join(here, "..", ".."))


def strip_self_mention(text: str) -> str:
    """★ **自述剔除**（同源先例：`check_id_set_diff.py` 剔除"自述告警段"）。

    为什么需要：本判据的**特征词**（「解冻」／`D16'`／"受检对象＝"）**会出现在 `BASELINE` 中
    "描述本判据自己"的那一行里** ⇒ 若不剔除，**判据会把自己所在的节误选为受检节**
    （**`T-049` 元层级表述纪律** · **自指污染第 12 例** · v0.328 实测抓获）。
    口径：**含本判据文件名的行**一律**不参与**受检节选择与段标记匹配。
    """
    keep = []
    for ln in text.splitlines():
        if "check_rewrite_pipeline" in ln:
            continue
        keep.append(ln)
    return "\n".join(keep)


def split_sections(text: str):
    """返回 [(标题, 索引, 正文)]；仅认 `## ` 顶层节（不含 `###`）。

    ★ **正文＝标题行之后的内容**（**不含标题行本身**）——
      否则形如「## 六十、回写轮」的**标题会让段④"回写"空过**（v0.328 自检当场抓出）。
    ★ 调用前**须先过 `strip_self_mention`**。
    """
    lines = text.splitlines()
    idx = [i for i, l in enumerate(lines) if l.startswith("## ") and not l.startswith("### ")]
    out = []
    for k, i in enumerate(idx):
        j = idx[k + 1] if k + 1 < len(idx) else len(lines)
        out.append((lines[i].strip(), k, "\n".join(lines[i + 1:j])))
    return out


def pick_target(sections):
    """最后一个含「解冻」或 `D16'` 的节索引；无则 None。"""
    hit = None
    for title, k, body in sections:
        if ("解冻" in body) or ("D16'" in body):
            hit = k
    return hit


def evaluate_sections(sections):
    """返回 (alerts, notes, target_title)"""
    alerts, notes = [], []
    if not sections:
        alerts.append("台账**未解析到任何顶层节** ⇒ 判据空转（★ 空转不得与通过同值）")
        return alerts, notes, None

    k = pick_target(sections)
    if k is None:
        notes.append("台账中**未找到含「解冻」或 `D16'` 的节** ⇒ 本判据**当期不适用**（非告警）")
        return alerts, notes, None

    title, _, body = sections[k]

    # ★★ 非回写轮豁免出口（`T-051` 边界"纯出稿／纯裁定轮不设五段要求"的机器出口）
    if SKIP_MARK in body:
        notes.append("受检节：%s" % title[:70])
        notes.append("   ⇒ 该节带**非回写轮豁免标记**（`%s`） ⇒ **当期不设五段要求**"
                     "（`T-051` 边界：纯出稿／纯裁定轮本无解冻—回写—重冻）" % SKIP_MARK)
        return alerts, notes, title

    # A 五段可追溯
    for seg, keys in MARK.items():
        if seg in ("①出稿", "②待裁"):
            window = "\n".join(sections[j][2]
                               for j in range(max(0, k - LOOKBACK), k + 1))
        else:
            window = body
        if not any(x in window for x in keys):
            where = ("本节或前 %d 节" % LOOKBACK) if seg in ("①出稿", "②待裁") else "本节"
            alerts.append("%s：**缺段 %s**（%s 内未见 %s）"
                          % (title[:40], seg, where, "／".join(keys)))

    # B 半冻检测
    if (("解冻" in body) or ("D16'" in body)) and not any(x in body for x in MARK["⑤重冻"]):
        alerts.append("%s：**半冻状态** —— 有「解冻」但**本节内无「重新冻结／重冻结」**"
                      "（`T-051` 规则④：单轮单次开闭、写完即冻）" % title[:40])

    notes.append("受检节：%s" % title[:70])
    for seg, keys in MARK.items():
        window = body
        if seg in ("①出稿", "②待裁"):
            window = "\n".join(sections[j][2] for j in range(max(0, k - LOOKBACK), k + 1))
        notes.append("   段 %s：%s" % (seg, "✅" if any(x in window for x in keys) else "❌"))
    return alerts, notes, title


def evaluate(repo: str):
    p = os.path.join(repo, "coordination", "BASELINE.md")
    if not os.path.isfile(p):
        return ["台账不存在：%s" % p], [], None
    with open(p, encoding="utf-8", errors="replace") as f:
        text = f.read()
    return evaluate_sections(split_sections(strip_self_mention(text)))


# ── 自检 ────────────────────────────────────────────────────────────────
# ★ 受检节须**只含段③④⑤**（段①② 由回看窗口提供）—— 夹具设计如此，方可测出回溯边界
TAIL = ("> `D16'` 解冻：已授权\n"
        "> 回写：已改底图\n"
        "> 重新冻结：已回 🔒\n")
FULL = "## 六十、本轮回写\n> 出稿：`coordination/reports/x.md`\n> 待裁：等发起人裁定\n" + TAIL
PARTIAL = "## 六十、本轮回写\n" + TAIL


def selftest():
    fails = []

    def run(label, text, want_alert, want_seg=None):
        a, _, _ = evaluate_sections(split_sections(text))
        got = bool(a)
        seg_ok = True if want_seg is None else any(("缺段 %s" % want_seg) in x for x in a)
        ok = (got == want_alert) and seg_ok
        print("  %s: %s%s" % (label, "PASS" if ok else "FAIL", "" if not a else "  ← %s" % a[0][:80]))
        if not ok:
            fails.append(label)

    run("侧①（阳性·五段齐全＋已重冻）", FULL, False)
    run("侧②（正例·缺段⑤重冻）", PARTIAL.replace("> 重新冻结：已回 🔒\n", ""), True, want_seg="⑤重冻")
    run("侧③（正例·缺段④回写）", PARTIAL.replace("> 回写：已改底图\n", ""), True, want_seg="④回写")
    run("侧④（反例·段①②在回看 3 节内）",
        "## 五十七、出稿裁轮\n> 出稿：x\n> 待裁：y\n"
        "## 五十八、其他\n中\n## 五十九、其他\n中\n" + PARTIAL, False)
    run("侧⑤（正例·段①②超出回看 3 节）",
        "## 五十五、出稿裁轮\n> 出稿：x\n> 待裁：y\n"
        "## 五十六、其他\n中\n## 五十七、其他\n中\n## 五十八、其他\n中\n## 五十九、其他\n中\n"
        + PARTIAL, True, want_seg="①出稿")

    # 侧⑦ 自述剔除（T-049 · 自指污染第 12 例）：含本判据文件名的行被剔除
    raw = "## 某节\n> 受检对象＝最后一个含「解冻」的节（见 check_rewrite_pipeline.py）\n"
    stripped = strip_self_mention(raw)
    ok = ("check_rewrite_pipeline" not in stripped) and ("解冻" not in stripped) and ("某节" in stripped)
    print("  侧⑦（反例·自述剔除去掉含判据名的行）: %s" % ("PASS" if ok else "FAIL"))
    if not ok:
        fails.append("侧⑦")

    # 侧⑧ ★ 非回写轮豁免出口（v0.330 轮新增）：受检节带标记 ⇒ 不告警（反例 · 豁免出口）
    skip_fixture = ("## 八十八、纯出稿轮\n"
                    "> 本节**只出稿、未改底图**；裁定后须走 `D16'` 类程序（**非回写轮**）\n"
                    + SKIP_MARK + "\n")
    a2, n2, t2 = evaluate_sections(split_sections(skip_fixture))
    ok = (not a2) and any("非回写轮豁免标记" in x for x in n2)
    print("  侧⑧（反例·非回写轮豁免出口 ⇒ 不告警）: %s%s"
          % ("PASS" if ok else "FAIL", "" if ok else "  ← %s" % (a2[:1] or n2[:1])))
    if not ok:
        fails.append("侧⑧")

    # 侧⑥ 回读真实仓库（C18）
    r = repo_root()
    a, notes, title = evaluate(r)
    ok = title is not None
    print("  侧⑥（对照·回读真实台账：受检节＝%s）: %s"
          % (title[:44] if title else "未定位到", "PASS" if ok else "FAIL"))
    if not ok:
        fails.append("侧⑥")

    print("=" * 62)
    if fails:
        print("自检结论：FAIL %d 项 %s" % (len(fails), fails))
        return 1
    print("自检结论：PASS（八侧：3 正例 ＋ 4 反例 ＋ 1 对照）")
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
    print("底图回写五段流水线判据（`T-051`）★ 先只告警，不判红")
    print("仓库根：%s" % repo)
    print("★ 受检对象＝台账**最后一个含「解冻」或 `D16'` 的节**（回看 %d 节供段①②）" % LOOKBACK)
    print("=" * 66)

    alerts, notes, _ = evaluate(repo)
    for n in notes:
        print("   %s" % n)

    print("\n" + "-" * 66)
    if alerts:
        print("⚠️ 告警 %d 条：" % len(alerts))
        for x in alerts:
            print("   - %s" % x)
    else:
        print("✅ 无告警（五段可追溯 ＋ 无半冻状态）")

    print("\n" + "=" * 66)
    print("告警合计：%d" % len(alerts))
    print("★ 本判据**先只告警、不判红**（退出码恒 0）；触发再评估＝告警命中**连续 N=5 轮为 0**。")
    print("★ 边界：只判**留痕是否齐**，不判「回写质量」；纯出稿轮／纯裁定轮**不设五段要求**。")
    print("=" * 66)
    return 0


if __name__ == "__main__":
    sys.exit(main())
