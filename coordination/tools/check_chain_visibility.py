#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""对话链可见性域判据（机制 31 的可执行载体 · ★ 先只告警、不判红 · v0.333 轮立项 · `T-063`）

【为什么】
  机制 31「对话链可见性域」经 **3 轮人工承载**（`C3-005`／`C3-006`／`C3-007`）后，
  结论（`T-063`）为：**人工承载足够起步、不足以守** ——
    3 轮俱"⛔ 项 0 处"，但**过滤由人工执行、无机器守** ⇒ 其"绿"是**人工背书**
    （`R83`／`R89` 同族：**"没人发现" ≠ "不存在"**；**"本轮看过" ≠ "以后不漏"**）。
  ⇒ 本脚本即其机器载体（用户 2026-09-24 裁定：**立**）。

════════════════════════════════════════════════════════════════════════
★★ 覆盖边界（★ 必须写在判据头注 —— 不得只写在报告里）
════════════════════════════════════════════════════════════════════════
  ① 本判据**只能扫「落盘版返回件」** —— 即 `coordination/reports/` 与
     `coordination/discussions/` 里**真实存在的文件**。
  ② **「只进对话、不落盘」的返回件，本判据无法覆盖** ——
     那部分内容**从未落盘**，机器**没有读的对象**（不是"漏扫"，是**对象不存在**）。
  ③ ⇒ **结论＝混合承载**：**机器守「落盘件」 ＋ 人工守「对话件」**。
  ④ ★★ **不得**因"本判据已立"就把**对话件当成已守** ——
     把未覆盖的部分读成"已覆盖"，正是本判据要防的那类**假绿**（`R83` 同族）。
  ⑤ ★ 既然守不住全部，本判据的"绿"**只证明一件事**：**已落盘的那几件里没扫到 ⛔ 形态**；
     **不证明**"本轮没有泄漏"。
════════════════════════════════════════════════════════════════════════

【一句话判据】
  对**受检件**（落盘返回件）：
    **规则 A**：扫描 ⛔ 六类形态 —— ① 本机绝对路径 ② 账号／口令 ③ 内网 IP
              ④ **端口号** ⑤ 密钥与 token ⑥ 二进制路径与体积。命中 ⇒ **告警**。
    **规则 B**：件内出现 **🔐 级内容**（实现细节形态）而**该链可见级别上限 < 🔐** ⇒ **告警**
              （须"屏蔽后拷"）。
  违反 ⇒ **告警**（**不判红** · 起步档位）。

【口径（七条）】
  1. **链上限唯一出处**：`coordination/security/chain_visibility.md` §一 登记表；
     本判据**只读、不另抄一份**（`C18`／`C19` 同源）。
  2. **受检件（默认）**＝ `git` 中 **HEAD 提交**改动过的 `reports/`／`discussions/` 文件；
     **若该集合为空** ⇒ **降级为"当轮日期件"**（文件名以 `BASELINE.md` **最后一个顶层节**标题里的
     `YYYY-MM-DD` 起首者），并**打印所用口径**（`R35`：报数必须写明口径）。
  3. **链判定**：先在件内**前 40 行**找 `C<n>` 形态；命中且该链已登记 ⇒ 用其上限；
     **否则用「已登记链中最严者」**（并打印该降级）。
  4. **源与链上限取更严者**（机制 31 §二 判据 ④）：本判据中"源"即 `C12` 四级别内容判定。
  5. **可豁免标记**：行内出现 `<!-- chain-vis-skip -->` ⇒ **跳过该行**
     （依 `T-041`：**剔除规则本身是正式规则**，且**每次扫描打印剔除行数**）。
  6. **不重算机器事实**（`C18`）：只读**文本形态**；故"合规"只等于"**文本里没扫到 ⛔ 形态**"，
     **不等于**"内容真的安全"（后者是人工守的部分）。
  7. ★ **边界（如实标注）**：**分级声明行**（同行含 🔐 字形，如本类报告的"🔐 屏蔽后拷：函数签名／…"）
     **属规范说明、不属泄漏** ⇒ **规则 B 排除**（与 `check_effect_column` 的"规范句排除"同法）。

【自检】
  `--selftest`：正反对照**十侧** ——
    ① 干净件 ⇒ 无告警（阳性对照）
    ② 本机绝对路径 ⇒ 告警（正例）
    ③ **端口号值** ⇒ 告警（正例）
    ④ 密钥／token ⇒ 告警（正例）
    ⑤ 二进制体积 ⇒ 告警（正例）
    ⑥ 含 `<!-- chain-vis-skip -->` ⇒ 不告警（反例 · 豁免出口）
    ⑦ 规则 B：含 `pub fn` 实现签名 而 链上限＝🔒 ⇒ 告警（正例）
    ⑧ 规则 B 反例：**分级声明行**（含 🔐 字形）⇒ **不告警**
    ⑨ 含 `C3` 链号 ⇒ 正确取到链上限（对照 · 同源读登记表）
    ⑩ 回读真实仓库：`security/chain_visibility.md` 能解析出 **≥1 条已登记链**（`C18` 回读）

【用法】
  check_chain_visibility.py                    # 实跑（**只告警**；退出码恒 0）
  check_chain_visibility.py --selftest         # 自检（十侧正反对照）
  check_chain_visibility.py --files a.md b.md  # 指定受检件
  check_chain_visibility.py --all              # 扫全部落盘返回件（人工审计用，告警会很多）
  check_chain_visibility.py --repo <path>

【触发再评估】
  告警命中**连续 N=5 轮为 0** ⇒ 再议转判红（与 `check_id_set_diff.py`／`check_effect_column.py` 同口径）。
  ★ 机制 31「判据后试用」**起点＝本轮（`v0.333` · `云内核-C3-008`）**，共 3 轮。
"""
from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys

SKIP_MARK = "<!-- chain-vis-skip -->"

# ── 规则 A：⛔ 六类形态（**只认"具体值"，不认"类别词"** —— 防大面积误报）───────────
ABS_PATH = re.compile(r"(?:[A-Za-z]:[\\/]{1,2}(?:Users|Documents|Desktop|AppData|Windows)"
                      r"|/{1,2}[cC]/[Uu]sers/)")
ACCOUNT = re.compile(r"(?:账号|帐号|用户名|账户|密码|password|passwd)\s*[:：=]\s*\S+")
LAN_IP = re.compile(r"\b(?:10|192\.168|172\.(?:1[6-9]|2\d|3[01]))\.\d{1,3}\.\d{1,3}\b")
PORT = re.compile(r"(?:端口|port)\s*[:：=]?\s*(\d{2,5})\b"
                  r"|\b(?:localhost|127\.0\.0\.1|\d{1,3}(?:\.\d{1,3}){3})\s*:\s*(\d{2,5})\b",
                  re.IGNORECASE)
SECRET = re.compile(r"(?:token|api[_-]?key|secret|passwd|sk-)[\s:=]{1,2}[A-Za-z0-9_\-]{8,}",
                    re.IGNORECASE)
BINARY = re.compile(r"(?:[A-Za-z]:|[\\/])[^\s|`]*\.(?:exe|dll|so|bin)\b"
                    r"|\d+(?:\.\d+)?\s*(?:MB|GB|兆字节)\b")

RULES_A = [
    ("①本机绝对路径", ABS_PATH, "⛔【本机绝对路径】"),
    ("②账号／口令", ACCOUNT, "⛔【账号／口令】"),
    ("③内网 IP", LAN_IP, "⛔【内网 IP】"),
    ("④端口号", PORT, "⛔【端口号】"),
    ("⑤密钥与 token", SECRET, "⛔【密钥与 token】"),
    ("⑥二进制路径与体积", BINARY, "⛔【二进制路径与体积】"),
]

# ── 规则 B：🔐 级内容（**实现细节形态**；分级声明行由 🔐 字形排除）──────────────
B_IMPL = [
    re.compile(r"\bpub\s+fn\s+\w+\s*\([^)]*\)\s*->"),
    re.compile(r"\bconst\s+[A-Z][A-Z0-9_]{2,}\s*:\s*(?:f32|f64|u8|u16|u32|u64|usize|i32)\s*=\s*[0-9]"),
    re.compile(r"\bGrade::T[0-3]\w*"),
]
LOCK_GLYPH = "🔐"
LIMIT_ORDER = {"🔓": 0, "🔒": 1, "🔐": 2, "⛔": 3}
LIMIT_NAME = {"🔓": "🔓 公开", "🔒": "🔒 半公开", "🔐": "🔐 保密", "⛔": "⛔ 机密"}
RE_CHAIN = re.compile(r"\bC(\d+)\b")


def repo_root() -> str:
    here = os.path.dirname(os.path.abspath(__file__))
    return os.path.abspath(os.path.join(here, "..", ".."))


# ── 链上限表（**唯一出处** · 同源读取）──────────────────────────────────────
def load_chain_limits(repo: str):
    """返回 { 'C3': ('🔒', 行文本摘要), ... } —— 只读登记表，不另抄一份。"""
    p = os.path.join(repo, "coordination", "security", "chain_visibility.md")
    out = {}
    if not os.path.isfile(p):
        return out
    with open(p, encoding="utf-8", errors="replace") as f:
        for i, ln in enumerate(f, 1):
            s = ln.strip()
            if not s.startswith("|"):
                continue
            cells = [c.strip() for c in s.strip("|").split("|")]
            if len(cells) < 3:
                continue
            m = re.fullmatch(r"\*{0,2}`?(C\d+)`?\*{0,2}", cells[0])
            if not m:
                continue
            glyphs = [g for g in ("⛔", "🔐", "🔒", "🔓") if g in cells[2]]
            if not glyphs:
                continue
            out[m.group(1)] = (glyphs[0], i)
            out.setdefault("__lines__", []).append((m.group(1), glyphs[0], i))
    return out


def strictest(glyphs):
    """取最严（数值最大）者；空 ⇒ None。"""
    if not glyphs:
        return None
    return max(glyphs, key=lambda g: LIMIT_ORDER.get(g, -1))


# ── 受检件选取 ─────────────────────────────────────────────────────────────
def git_changed_return_files(repo: str):
    try:
        p = subprocess.run(["git", "show", "--name-only", "--pretty=format:", "HEAD"],
                           cwd=repo, capture_output=True, text=True, timeout=30)
    except Exception:
        return None, "git 不可用"
    if p.returncode != 0:
        return None, "git show 失败"
    names = [x.strip() for x in (p.stdout or "").splitlines() if x.strip()]
    hits = [n for n in names
            if n.startswith(("coordination/reports/", "coordination/discussions/"))
            and n.endswith(".md")]
    return hits, "HEAD 提交改动集"


def current_round_date(repo: str):
    p = os.path.join(repo, "coordination", "BASELINE.md")
    if not os.path.isfile(p):
        return None
    date = None
    with open(p, encoding="utf-8", errors="replace") as f:
        for ln in f:
            if ln.startswith("## ") and not ln.startswith("### "):
                m = re.search(r"(20\d{2}-\d{2}-\d{2})", ln)
                if m:
                    date = m.group(1)
    return date


def date_scope_files(repo: str):
    date = current_round_date(repo)
    if not date:
        return [], None
    hits = []
    for sub in ("reports", "discussions"):
        d = os.path.join(repo, "coordination", sub)
        if not os.path.isdir(d):
            continue
        for fn in sorted(os.listdir(d)):
            if fn.startswith(date) and fn.endswith(".md"):
                hits.append("coordination/%s/%s" % (sub, fn))
    return hits, date


def all_return_files(repo: str):
    hits = []
    for sub in ("reports", "discussions"):
        d = os.path.join(repo, "coordination", sub)
        if not os.path.isdir(d):
            continue
        for fn in sorted(os.listdir(d)):
            if fn.endswith(".md"):
                hits.append("coordination/%s/%s" % (sub, fn))
    return hits


# ── 单件检查 ───────────────────────────────────────────────────────────────
def check_file(repo: str, rel: str, limits: dict, default_limit: str):
    """返回 (alerts, skipped_lines, chain_used, limit_used)"""
    path = os.path.join(repo, rel)
    if not os.path.isfile(path):
        return ["%s：**文件不存在**（受检件选取有误？）" % rel], 0, None, default_limit
    with open(path, encoding="utf-8", errors="replace") as f:
        lines = f.read().splitlines()

    # 链判定：前 40 行找 C<n>
    chain_used, limit_used = None, default_limit
    head = "\n".join(lines[:40])
    for m in RE_CHAIN.finditer(head):
        cid = "C" + m.group(1)
        if cid in limits:
            chain_used = cid
            limit_used = limits[cid][0]
            break

    alerts, skipped = [], 0
    base = os.path.basename(rel)
    for ln, line in enumerate(lines, 1):
        if SKIP_MARK in line:
            skipped += 1
            continue
        # 规则 A
        for label, rx, tag in RULES_A:
            mm = rx.search(line)
            if mm:
                alerts.append("%s 行%d：%s（命中：`%s`）" % (base, ln, tag, mm.group(0)[:40]))
        # 规则 B（★ 排除分级声明行：同行含 🔐 字形 ⇒ 是本类报告的规范说明，非泄漏）
        if LIMIT_ORDER.get(limit_used, 1) < LIMIT_ORDER[LOCK_GLYPH] and LOCK_GLYPH not in line:
            for rx in B_IMPL:
                if rx.search(line):
                    alerts.append("%s 行%d：**🔐 级内容超链上限**（本链上限 %s < 🔐；须屏蔽后拷）"
                                  % (base, ln, LIMIT_NAME.get(limit_used, limit_used)))
                    break
    return alerts, skipped, chain_used, limit_used


def evaluate(repo: str, files=None, use_all=False):
    limits = load_chain_limits(repo)
    glyphs = [v[0] for k, v in limits.items() if k != "__lines__"]
    default_limit = strictest(glyphs) or "🔒"

    if files:
        chosen, scope_note = list(files), "CLI 指定"
    elif use_all:
        chosen, scope_note = all_return_files(repo), "全量（人工审计）"
    else:
        hits, note = git_changed_return_files(repo)
        if hits:
            chosen, scope_note = hits, note
        else:
            chosen, date = date_scope_files(repo)
            scope_note = "降级·当轮日期件（%s）%s" % (date or "未读到日期",
                                                 "" if hits is None else "；HEAD 改动集为空")
    alerts, skipped_total = [], 0
    for rel in chosen:
        a, sk, cu, lu = check_file(repo, rel, limits, default_limit)
        alerts.extend(a)
        skipped_total += sk
    return alerts, chosen, scope_note, limits, default_limit, skipped_total


# ── 自检 ───────────────────────────────────────────────────────────────────
CLEAN = ("# 干净件\n\n本件只写层级映射与治理编号。示例：`C3` 链、`v0.333`、crate 名 `npb-gateway`。\n")
CASE_ABS = CLEAN + "备份在 C:\\Users\\someone\\Documents\\x.md 下。\n"
CASE_PORT = CLEAN + "网关默认端口 3000，用法见注释。\n"
CASE_SECRET = CLEAN + "请求头带 token=abcdef1234567890 即可。\n"
CASE_SIZE = CLEAN + "二进制体积约 15.8 MB。\n"
CASE_SKIP = CLEAN + "端口 9999 " + SKIP_MARK + " （显式豁免行）\n"
CASE_B = CLEAN + "实现：`pub fn confirm_receipt(&self, r: Receipt) -> ExecResult`\n"
CASE_B_DECL = CLEAN + "🔐 **屏蔽后拷**：函数签名／阈值常量／T0–T3 判定顺序。\n"


def selftest():
    import tempfile

    fails = []
    repo = repo_root()
    limits = load_chain_limits(repo)
    glyphs = [v[0] for k, v in limits.items() if k != "__lines__"]
    default_limit = strictest(glyphs) or "🔒"

    def run(label, content, want_alert, want_sub=None):
        d = tempfile.mkdtemp()
        p = os.path.join(d, "t.md")
        with open(p, "w", encoding="utf-8", newline="\n") as f:
            f.write(content)
        alerts, _sk, _cu, _lu = check_file(d, "t.md", limits, default_limit)
        got = bool(alerts)
        sub_ok = True if want_sub is None else any(want_sub in x for x in alerts)
        ok = (got == want_alert) and sub_ok
        print("  %s: %s%s" % (label, "PASS" if ok else "FAIL",
                              "" if not alerts else "  ← %s" % alerts[0][:84]))
        if not ok:
            fails.append(label)

    run("侧①（阳性·干净件）", CLEAN, False)
    run("侧②（正例·本机绝对路径）", CASE_ABS, True, want_sub="本机绝对路径")
    run("侧③（正例·端口号值）", CASE_PORT, True, want_sub="端口号")
    run("侧④（正例·密钥／token）", CASE_SECRET, True, want_sub="密钥与 token")
    run("侧⑤（正例·二进制体积）", CASE_SIZE, True, want_sub="二进制路径与体积")
    run("侧⑥（反例·豁免标记）", CASE_SKIP, False)
    run("侧⑦（正例·规则B 实现签名超上限）", CASE_B, True, want_sub="超链上限")
    run("侧⑧（反例·分级声明行含 🔐 ⇒ 不报）", CASE_B_DECL, False)

    # 侧⑨ 链号解析（对照 · 同源）
    d = tempfile.mkdtemp()
    p = os.path.join(d, "t.md")
    with open(p, "w", encoding="utf-8", newline="\n") as f:
        f.write("# 件\n\n本链 C3｜指令序号 云内核-C3-008。\n")
    _a, _sk, cu, lu = check_file(d, "t.md", limits, default_limit)
    ok9 = (cu == "C3")
    print("  侧⑨（对照·链号解析：取到 %s ／ 上限 %s）: %s"
          % (cu, LIMIT_NAME.get(lu, lu), "PASS" if ok9 else "FAIL"))
    if not ok9:
        fails.append("侧⑨")

    # 侧⑩ 回读真实仓库（C18）
    n = len([k for k in limits if k != "__lines__"])
    ok10 = n >= 1
    print("  侧⑩（对照·回读真实登记表：解析出已登记链 %d 条 ／ 最严上限 %s）: %s"
          % (n, LIMIT_NAME.get(default_limit, default_limit), "PASS" if ok10 else "FAIL"))
    if not ok10:
        fails.append("侧⑩")

    print("=" * 62)
    if fails:
        print("自检结论：FAIL %d 项 %s" % (len(fails), fails))
        return 1
    print("自检结论：PASS（十侧：6 正例 ＋ 3 反例 ＋ 1 对照 ＋ 回读真实登记表）")
    return 0


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--repo", default=repo_root())
    ap.add_argument("--selftest", action="store_true")
    ap.add_argument("--files", nargs="*", default=None)
    ap.add_argument("--all", action="store_true")
    a = ap.parse_args()

    if a.selftest:
        return selftest()

    repo = os.path.abspath(a.repo)
    print("=" * 70)
    print("对话链可见性域判据（机制 31 ／ `T-063`）★ 先只告警，不判红")
    print("仓库根：%s" % repo)
    print("=" * 70)
    print()
    print("★★ 覆盖边界（写在头注，此处复述）")
    print("   · 只扫「落盘版返回件」（`reports/` ＋ `discussions/`）")
    print("   · 「只进对话、不落盘」的返回件**无法覆盖**（对象不存在）")
    print("   · ⇒ **混合承载**：机器守落盘件 ＋ 人工守对话件")
    print("   · ★ 不得因本判据已立，就把对话件当成已守")
    print("   · ★ 本判据的「绿」只证明：**已落盘那几件里没扫到 ⛔ 形态**")

    alerts, chosen, scope_note, limits, default_limit, skipped = evaluate(
        repo, files=a.files, use_all=a.all)

    n_chain = len([k for k in limits if k != "__lines__"])
    print("\n链上限表（**唯一出处** `coordination/security/chain_visibility.md`）：已登记 %d 条"
          % n_chain)
    for cid, g, i in limits.get("__lines__", []):
        print("   · %s ⇒ 上限 %s（行 %d）" % (cid, LIMIT_NAME.get(g, g), i))
    print("★ 默认上限（未在件内识别到链号时取「已登记链中最严者」）＝ %s"
          % LIMIT_NAME.get(default_limit, default_limit))
    print("\n受检件口径：%s ／ 共 %d 份" % (scope_note, len(chosen)))
    for rel in chosen[:12]:
        print("   · %s" % rel)
    if len(chosen) > 12:
        print("   · …（其余 %d 份略）" % (len(chosen) - 12))
    if skipped:
        print("  [已剔除 · 含豁免标记 %d 行]（`T-041`：**剔除规则本身是正式规则**；标记＝`%s`）"
              % (skipped, SKIP_MARK))

    print("\n" + "-" * 70)
    if alerts:
        print("⚠️ 告警 %d 条：" % len(alerts))
        for x in alerts:
            print("   - %s" % x)
    else:
        print("✅ 无告警（受检件文本中未扫到 ⛔ 六类形态；亦未见 🔐 级内容超链上限）")

    print("\n" + "=" * 70)
    print("告警合计：%d" % len(alerts))
    print("★ 本判据**先只告警、不判红**（退出码恒 0）；触发再评估＝告警命中**连续 N=5 轮为 0**。")
    print("★ 机制 31「判据后试用」起点＝`v0.333`·`云内核-C3-008`；共 3 轮 ⇒ 再评点＝3 轮后。")
    print("=" * 70)
    return 0


if __name__ == "__main__":
    sys.exit(main())
