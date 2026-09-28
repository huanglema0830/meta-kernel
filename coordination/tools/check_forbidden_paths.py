#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""机制 20 禁令③「公开层不得写真实代码路径」判据（★ 先只告警 · v0.333 第二十八补立项）

【立项】用户 2026-09-28 `云内核-C3-030` §四 裁定：**授权实现甲案 —— 只立 ③（真实代码路径）**。
        设计依据：`coordination/discussions/2026-09-28_机制20禁令判据设计稿.md` §五 方案甲。
【不做】① 原名 ／ ② 原名全拼（**守禁区⑪**）；**不判红**（**先只告警**）；**不触 `C12`**
        （**全程只用仓库内公开信息，不读仓库外加密层**）。

【为什么只有 ③ 可判】
  机制 20 公开层三条禁令（`TERMS.md` 第 5 行）：**原名**／**原名的全拼**／**真实代码路径**。
  其中 ① 原名 与 ② 全拼的「真值」**只存在于仓库外的加密内容层** ⇒ 判据输入**不在判据可达范围**
  （等价于「要判有没有泄露，先得读私有层」，而读私有层本身就触 `C12`）。
  ⇒ **只有 ③「真实代码路径」可判** —— 其真值＝仓库内**实际存在的相对路径**，可由 `git ls-files` **完整枚举**。

【一句话判据】
  扫 `coordination/TERMS.md`（公开层）正文，提取**路径型 token**（含 `/` 且以扩展名结尾），
  与 `git ls-files`（真实仓库路径集合）**精确取交集** ⇒ **命中即告警**。
  ★ 规则极简：判据问的不是「这句话是不是在举例」，而是「**这个串是否真存在于本仓库**」 ——
  **凡命中真路径者，无论是否在举例，都已构成「公开层写出了真实代码路径」**。
  ★ 该取向与 `check_judge_gaps` 的「**自述剔除**」**相反**（`R28` 正是「举例式泄露」这一形态：
  为讲禁令而把真串写进公开层 ⇒ **不该剔除，该抓**）。

【口径（五条）】
  1. **范围**：**只扫 `coordination/TERMS.md`**（公开层唯一载体；同设计稿 §2.1 候选「甲」）。
  2. **真值来源**：`git -c core.quotepath=false ls-files`（**`C18`：锚点取真实仓库**，
     **不手写第二份路径清单**；`core.quotepath=false` 口径已在历轮固化，防中文路径被转义）。
  3. **检测**：只取**含 `/` 且以扩展名结尾**的 token（`docs/` 这类目录名**不取** ⇒ 防误报）；
     与真值**精确相等**比对（**不做后缀/前缀模糊匹配** ⇒ 零误报优先）。
  4. ★ **负向白名单**（**用户于 `C3-030` §4.2 授权「具体白名单由你定」** · **两层、均为"路径级"**）：
       **W1** `{PRIVATE_ASSETS}/…` —— **仓库外占位符**（本就不在 `ls-files` 中；**显式剔除以防未来**）。
       **W2** ★★ **路径级**（**不是节级**）：**凡在 `§一之二`／`§一之三`／`§一之四` 三节内
            被正当登记为「仓库内公开文档」的真实路径** ⇒ **全文任何位置复述均豁免**。
             **依据**：`TERMS.md` **自身明文声明**三节为「**非换壳**」且「**指针指向仓库内公开文档**」；
             ★ **`§六 更新记录`复述同一指针不构成泄露**（它明说这是允许的公开指针）——
             此点由**首跑实测暴露**（首版按"节级"白名单 ⇒ `§六` 两处复述被误判为告警）。
     ★★ **为什么不豁免"整节"**：若按节豁免 `§六`，则**任何**写进更新记录的真路径都会漏判 ⇒
        **W2 取"路径级"**：唯有**先在非换壳节被显式登记**的路径才豁免 ⇒ **未被登记的真路径仍必告警**
        （`--selftest` 侧⑥ 专门证明这一点）。
     ★★ 判据**同时打印「未过滤命中数」与「白名单内命中数」** ⇒ **白名单的代价当场可见、可被复核**
        （诚实标注：白名单不是隐藏，是**显式口径**；`--list-whitelist` 可打印豁免口径）。
  5. **效力**：**只告警、不判红**（**rc 恒 0**）；命中清单**逐条可核**（行号 ＋ token ＋ 原行摘要）。

【自检 `--selftest`】正反对照**七侧**（★ 反例**从真实仓库取真路径**，不造串 —— 避 `R64` 夹具同错）：
  ① 回读真实仓库：`git ls-files` 条数 > 0，且 `TERMS.md` 存在（**C18**）
  ② **正例**：仅含 `{PRIVATE_ASSETS}/…` 占位符的文本 ⇒ **0 告警**（W1 生效）
  ③ **反例**：**取真实仓库中一个真路径**注入「§一」区 ⇒ **必告警**（判据非空转）
  ④ **反例**：同一真路径放在「§一之二」区（**非换壳节**）⇒ **不告警**（W2 生效）
  ⑤ **反例**：**目录形 token**（如 `docs/`，无扩展名）⇒ **不命中**（防误报口径生效）
  ⑥ **反例**：同一真路径**只**出现在「§六 更新记录」（**非**非换壳节）⇒ **仍告警**
        （★ 证明 W2 是**路径级**、**不是**"把 `§六` 整节放宽"）
  ⑦ 实跑 `TERMS.md`：**回读真实公开层**并打印结果（未过滤／白名单内／告警合计／W2 集合）

【用法】
  check_forbidden_paths.py                    # 实跑（**只告警**；退出码恒 0）
  check_forbidden_paths.py --selftest          # 自检（六侧正反对照）
  check_forbidden_paths.py --repo <path>
  check_forbidden_paths.py --list-whitelist    # 打印负向白名单口径

【触发再评估】
  告警命中**连续 N=5 轮为 0** ⇒ 再议（与 `check_id_set_diff.py`／`check_judge_gaps.py` 同口径）。
  ★ 但本判据的**价值在"未来"**：当下 0 告警＝**公开层守住了**，属正常态、**不等于空转**
  （空转与否由 `--selftest` 侧③ 的正例/反例对照证明）。
"""
from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys

TERMS_REL = "coordination/TERMS.md"

# ★ 只取「含 / 且以扩展名结尾」的 token；含中文范围以覆盖本仓库既有中文路径名
RE_TOKEN = re.compile(r"[A-Za-z0-9_\u4e00-\u9fff][A-Za-z0-9_./\-\u4e00-\u9fff]*\.[A-Za-z0-9]{1,6}")
# ★ W1：仓库外占位符整体剔除（先剔除，再提 token ⇒ 避免 token 从占位符内部起算）
RE_PRIVATE_PLACEHOLDER = re.compile(r"\{PRIVATE_ASSETS\}/[^\s`\)\|，。；]+")

# ★ W2：非换壳节（`TERMS.md` 自身明文声明「指针指向仓库内公开文档」）
WL_SECTION_HEADS = ("## 一之二", "## 一之三", "## 一之四")
WL_SECTION_END = "## 二"


def repo_root() -> str:
    here = os.path.dirname(os.path.abspath(__file__))
    return os.path.abspath(os.path.join(here, "..", ".."))


def ls_files(repo: str):
    """真实仓库路径集合（`C18`：锚点取真实仓库）。返回 (set, rc, err)。"""
    try:
        p = subprocess.run(
            ["git", "-c", "core.quotepath=false", "ls-files"],
            cwd=repo, capture_output=True, encoding="utf-8", errors="replace",
        )
    except OSError as e:
        return None, 127, str(e)
    if p.returncode != 0:
        return None, p.returncode, (p.stderr or "").strip()[:200]
    return {ln.strip() for ln in (p.stdout or "").splitlines() if ln.strip()}, 0, ""


def split_whitelist(line: str):
    """返回 (剔除占位符后的行, W1 剔除数)。"""
    n = len(RE_PRIVATE_PLACEHOLDER.findall(line))
    return RE_PRIVATE_PLACEHOLDER.sub(" ", line), n


def extract_tokens(line: str):
    out = []
    for m in RE_TOKEN.finditer(line):
        tok = m.group(0)
        if "/" in tok:          # ★ 口径 3：只认含 / 的路径型 token
            out.append(tok)
    return out


def collect_wl_paths(lines, real_paths):
    """★ 第 1 趟：扫「**非换壳节**」区，收集被正当登记为「仓库内公开文档」的真实路径 ⇒ **W2 集合**。

    口径（**路径级**）：只有**先在非换壳节被显式登记**的路径才进入 W2 ⇒ 全文任何位置复述均豁免；
    **未被任何非换壳节登记**的真实路径 ⇒ **不豁免** ⇒ 仍告警（`C18`：同一状态机，不另写第二份）。
    """
    wl = set()
    in_wl = False
    for raw in lines:
        s = raw.rstrip("\n")
        if s.startswith("## "):
            if any(s.startswith(h) for h in WL_SECTION_HEADS):
                in_wl = True
            elif s.startswith(WL_SECTION_END):
                in_wl = False
        if not in_wl:
            continue
        cleaned, _ = split_whitelist(s)
        for tok in extract_tokens(cleaned):
            if tok in real_paths:
                wl.add(tok)
    return wl


def scan_text(lines, real_paths, wl_paths=None):
    """扫一组「行」（列表[文本]）；返回统计与命中明细。★ 与实跑**同一函数**（`C18`）。"""
    if wl_paths is None:
        wl_paths = collect_wl_paths(lines, real_paths)
    w1_total = 0
    hits = []           # (lineno, token, 说明, whitelisted:bool)
    in_wl = False
    for i, raw in enumerate(lines, 1):
        s = raw.rstrip("\n")
        if s.startswith("## "):
            if any(s.startswith(h) for h in WL_SECTION_HEADS):
                in_wl = True
            elif s.startswith(WL_SECTION_END):
                in_wl = False
        cleaned, w1 = split_whitelist(s)
        w1_total += w1
        for tok in extract_tokens(cleaned):
            if tok not in real_paths:
                continue
            if tok in wl_paths:
                hits.append((i, tok, "W2·非换壳已登记" if in_wl else "W2·他处复述", True))
            else:
                hits.append((i, tok, "换壳(受约束)", False))
    wl_hits = [h for h in hits if h[3]]
    alerts = [h for h in hits if not h[3]]
    return {
        "w1": w1_total,
        "raw": len(hits),
        "wl": len(wl_hits),
        "alerts": alerts,
        "hits": hits,
        "wl_paths": sorted(wl_paths),
    }


def scan(repo: str):
    real, rc, err = ls_files(repo)
    if rc != 0:
        return None, rc, "git ls-files 失败：%s" % err
    terms = os.path.join(repo, TERMS_REL)
    if not os.path.isfile(terms):
        return None, 2, "公开层文件不存在：%s" % TERMS_REL
    with open(terms, "r", encoding="utf-8") as f:
        lines = f.read().splitlines()
    res = scan_text(lines, real)
    res["real_n"] = len(real)
    res["lines_n"] = len(lines)
    return res, 0, ""


# ────────────────────────────── 实跑 ──────────────────────────────
def do_run(repo: str) -> int:
    print("机制 20 禁令③ 判据（公开层不得写真实代码路径）｜repo = %s" % repo)
    print("★ 口径：只扫 %s；真值＝git ls-files；含 / 且带扩展名；精确相等；先只告警" % TERMS_REL)
    res, rc, err = scan(repo)
    if res is None:
        print("❌ 无法评估：%s" % err)
        return rc
    print("真值来源：git ls-files ⇒ %d 条真实仓库路径" % res["real_n"])
    print("扫描对象：%s ⇒ 共 %d 行" % (TERMS_REL, res["lines_n"]))
    print("W1 剔除（仓库外占位符 {PRIVATE_ASSETS}/…）：%d 处" % res["w1"])
    print("W2 集合（非换壳节内正当登记的公开文档路径）：%d 条" % len(res["wl_paths"]))
    for p in res["wl_paths"]:
        print("      - %s" % p)
    print("未过滤命中（命中真实仓库路径的总次数）：%d" % res["raw"])
    print("其中白名单内（W1/W2 · 不计告警）：%d" % res["wl"])
    print("告警合计：%d" % len(res["alerts"]))
    if res["hits"]:
        print("---- 命中明细（含白名单内，供复核）----")
        for lineno, tok, sec, wl in res["hits"]:
            print("  行 %-4d [%s]%s %s"
                  % (lineno, sec, "（白名单·不计）" if wl else " ★告警", tok))
    if res["alerts"]:
        print("\n★ 告警：公开层（换壳术语区）出现了仓库内**真实存在的代码路径** ——")
        print("  ⇒ 须改为**占位串**（如 `<某真实路径>`）或**代码代号**（`CODE-xxx`），见 `R28` 教训。")
    else:
        print("\n⇒ ✅ 告警 0（公开层换壳区未见真实代码路径）")
    print("★ 提醒：本判据**只告警**（rc 恒 0）⇒ 告警数须**另行读本输出**，不能只看退出码（`R83`）。")
    return 0


def do_list_whitelist() -> int:
    print("★ 负向白名单（口径，显式列出以便复核）—— **两层、均为路径级**：")
    print("  W1  {PRIVATE_ASSETS}/…  —— 仓库外占位符（不在 ls-files 中；显式剔除以防未来）")
    print("  W2  **路径级**：凡在 %s 三节内被正当登记为「仓库内公开文档」的真实路径"
          % " ／ ".join(WL_SECTION_HEADS))
    print("      ⇒ 全文任何位置复述均豁免；★ **未被这些节登记的真路径仍必告警**（不放宽）")
    print("  ★ 白名单**只做剔除、不做放宽**：命中明细仍全部打印（见实跑输出）。")
    return 0


# ────────────────────────────── 自检 ──────────────────────────────
def do_selftest(repo: str) -> int:
    ok = True
    print("check_forbidden_paths 自检（正反对照 · 七侧 · 反例取自真实仓库）")
    print("")

    real, rc, err = ls_files(repo)
    if real is None:
        print("[侧①] ❌ git ls-files 失败 rc=%d %s" % (rc, err))
        print("\n自检结论 = FAIL")
        return 1
    terms_ok = os.path.isfile(os.path.join(repo, TERMS_REL))
    print("[侧①] 回读真实仓库：ls-files = %d 条｜%s 存在 = %s（C18）"
          % (len(real), TERMS_REL, terms_ok))
    if not (len(real) > 0 and terms_ok):
        ok = False
        print("       ⇒ ❌ 真值或公开层缺失")
    else:
        print("       ⇒ ✅")

    # ★ 从真实仓库取一个真路径当反例（不造串 ⇒ 避 R64 夹具同错）
    sample = None
    for cand in sorted(real):
        if cand.endswith(".md") and "/" in cand:
            sample = cand
            break
    if sample is None:
        sample = sorted(real)[0] if real else None
    print("[侧①-b] 反例取用的真路径（取自 ls-files）：%s" % sample)
    if not sample:
        ok = False

    # 侧② 正例：仅占位符 ⇒ 0 告警
    r2 = scan_text(["| `TERM-001` | **X** | 说明 | `{PRIVATE_ASSETS}/terms/TERM-001.md.enc` |"],
                   real)
    good2 = (len(r2["alerts"]) == 0 and r2["w1"] == 1)
    print("[侧②] 正例（仅 W1 占位符）⇒ 告警 %d ｜W1 剔除 %d ⇒ %s"
          % (len(r2["alerts"]), r2["w1"], "✅" if good2 else "❌"))
    ok = ok and good2

    # 侧③ 反例：真路径放「§一（换壳区）」⇒ 必告警
    r3 = scan_text(["## 一、术语表", "| `TERM-001` | **X** | 说明 | `%s` |" % sample], real)
    good3 = (len(r3["alerts"]) == 1)
    print("[侧③] 反例（真路径置于§一·换壳区）⇒ 告警 %d ⇒ %s"
          % (len(r3["alerts"]), "✅ 非空转" if good3 else "❌ 竟未告警"))
    ok = ok and good3

    # 侧④ 反例：同一真路径放「§一之二（非换壳区）」⇒ 不告警（W2 生效）
    r4 = scan_text(["## 一之二、接口与引用类术语", "| `TERM-006` | **X** | 说明 | `%s` |" % sample],
                   real)
    good4 = (len(r4["alerts"]) == 0 and r4["wl"] == 1)
    print("[侧④] 反例（同真路径置于§一之二·非换壳节）⇒ 告警 %d｜白名单内 %d ⇒ %s"
          % (len(r4["alerts"]), r4["wl"], "✅ W2 生效" if good4 else "❌ 白名单边界失效"))
    ok = ok and good4

    # 侧⑤ 反例：目录形 token（无扩展名）⇒ 不命中
    r5 = scan_text(["## 一、术语表", "见 `docs/` 与 `coordination/` 两处（目录名，非路径文件）"], real)
    good5 = (len(r5["hits"]) == 0)
    print("[侧⑤] 反例（目录形 token `docs/`）⇒ 命中 %d ⇒ %s"
          % (len(r5["hits"]), "✅ 防误报生效" if good5 else "❌ 目录名被误报"))
    ok = ok and good5

    # 侧⑥ 反例：同一真路径**只**出现在「§六 更新记录」（非非换壳节）⇒ 仍告警
    #        ★ 证明 W2 是**路径级**、不是"把 §六 整节放宽"
    r6 = scan_text(["## 一、术语表", "| `TERM-001` | **X** | 说明 | 占位 |",
                    "## 六、更新记录", "| 2026-01-01 | 变更：见 `%s` |" % sample], real)
    good6 = (len(r6["alerts"]) == 1)
    print("[侧⑥] 反例（同真路径只置于§六·非非换壳节）⇒ 告警 %d ⇒ %s"
          % (len(r6["alerts"]), "✅ 路径级 W2（未放宽 §六）" if good6 else "❌ §六 被整节放宽"))
    ok = ok and good6

    # 侧⑦ 实跑真实公开层
    res, rc6, err6 = scan(repo)
    if res is None:
        ok = False
        print("[侧⑦] ❌ 实跑失败：%s" % err6)
    else:
        print("[侧⑦] 实跑真实 %s：未过滤 %d｜白名单内 %d｜**告警 %d**｜W2 集合 %d 条"
              % (TERMS_REL, res["raw"], res["wl"], len(res["alerts"]), len(res["wl_paths"])))
        for lineno, tok, sec, wl in res["hits"]:
            print("       行 %-4d [%s]%s %s" % (lineno, sec, "（白名单）" if wl else " ★", tok))

    print("\n自检结论 =", "PASS" if ok else "FAIL")
    return 0 if ok else 1


def main() -> int:
    ap = argparse.ArgumentParser(description="机制 20 禁令③：公开层不得写真实代码路径（只告警）")
    ap.add_argument("--selftest", action="store_true", help="自检（六侧正反对照）")
    ap.add_argument("--list-whitelist", action="store_true", help="打印负向白名单口径")
    ap.add_argument("--repo", default=None, help="仓库根（默认自动定位）")
    a = ap.parse_args()

    repo = a.repo or repo_root()
    if a.list_whitelist:
        return do_list_whitelist()
    if a.selftest:
        return do_selftest(repo)
    return do_run(repo)


if __name__ == "__main__":
    sys.exit(main())
