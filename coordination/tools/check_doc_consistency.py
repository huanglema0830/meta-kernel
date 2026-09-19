#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""机制 25 · 文档一致性判据（check_doc_consistency.py）

【为什么需要它】
R69／R72 家族：**"迁移进度"这类状态型文档会静默过期，且没有任何判据在检查它**。
R62 是"判据盲区"（判据只证明没多改字、不证明改了的字对）；本判据补的是它的**文档侧同族**：
**文档说的状态，与机器实测的状态，是否一致**。

【三条判据】
P1 · **段号一致性（文档 ↔ 机器）**：逐文件取"活跃段号陈述"的**最大值**，须等于机器实测最大段号。
     —— 口径说明（R35）：取"每文件最大值"而非"逐条比对"，因为段号是**单调递增的历史叙事**
     （文档会记录"扩到第 ⑥ 段→⑧→⑩"的过程）；**落后的是"最大值"，不是每一条**。
P2 · **收口一致性（文档 ↔ 机器 · ★ R79 起为「双侧」）**：
     ① **P2a**：机器**已收口** ⇒ 文档不得仍有"剩余 N 片"的**活跃**陈述；
     ② **P2b**：机器**未收口** ⇒ 文档不得称"已收口"（**反向**，R79 前缺失该侧 ⇒ 单侧盲区）。
P3 · **文档内部一致性（同文件自洽）**：同一文件内既有"已收口"又有活跃"剩余 N 片"⇒ 自相矛盾。

【活跃 vs 历史留痕】
含下列**历史标记词**的行视为**历史留痕**，不参与 P1/P2/P3：
    修订前（同义：改前） / 原为 / 原文 / 已处置 / 已修订 / 订正 / 引述 / 当时 / 历史 / ~~（删除线）
理由：登记表本身就是"引述被修订的原文"，若不排除，判据会把自己的**登记**判红。

【扫描范围】
`coordination/*.md`（顶层治理文件）＋ `README.md` ＋ `docs/*.md`。
**排除** `coordination/reports/`、`coordination/discussions/`：它们是**当轮快照**（自陈基准版本），
天然落后于当前机器状态，纳入会造成常亮红灯。

【自检（按 C18：同源 ＋ 回读真实仓库 ＋ 可区分"0"与"解析失败"）】
① 真实仓库锚点：`docs/` 与 `coordination/` 顶层必须解析到 >0 个文件；
② 机器事实源：`verify.rs` 必须解析到 >=5 个段标题（否则判据空转）；
③ 夹具·一致文档 ⇒ 不判红；④ 夹具·段号落后 ⇒ 判红；⑤ 夹具·收口却说剩余 ⇒ 判红；
⑥ 夹具·未收口却说已收口 ⇒ 判红（P2b 反向侧）。

【★ 口径同源（C18 · R79 修复）】
本判据的"机器事实·迁移收口"**直接 import** `check_migration_closure.py::modules()`，
**不另写第二份枚举**。此前用 `glob("*.rs")`（**仅顶层**）⇒ 与 closure 的递归口径差 **9 个模块**
（源 47 vs 56：漏 `l4/*`＋`l7/*` 共 8 个 + `lib` 归一差异），且**同一事实两个数** ⇒ 二义性。

用法：
    python coordination/tools/check_doc_consistency.py            # 实跑
    python coordination/tools/check_doc_consistency.py --selftest # 六侧自检
    python coordination/tools/check_doc_consistency.py --list     # 只列活跃陈述，不作判
"""
import argparse
import io
import os
import re
import sys
import tempfile
from pathlib import Path

# ---------- ★ 口径同源（C18 · R79）：机器事实的枚举**直接复用 closure 的实现** ----------
# 不另写第二份枚举 —— 否则「同一事实两个数」（R79 的根因）。
_TOOLS_DIR = Path(__file__).resolve().parent
if str(_TOOLS_DIR) not in sys.path:
    sys.path.insert(0, str(_TOOLS_DIR))
try:
    from check_migration_closure import modules as _closure_modules  # noqa: E402
    _CLOSURE_OK = True
except Exception:  # pragma: no cover - 仅在工具缺失时退化
    _closure_modules = None
    _CLOSURE_OK = False

# ---------- 常量（唯一权威，自检夹具复用同一份 ⇒ C18 同源）----------
VERIFY_REL = "meta-kernel-boot/kernel/src/verify.rs"
SRC_DIR = "meta-kernel-core/src"
TGT_DIR = "meta-kernel-core-nostd/src"

# 历史标记词：含这些词的行 = 历史留痕，不参与判据
HISTORY_MARKS = ["修订前", "改前", "原为", "原文", "已处置", "已修订", "订正",
                 "引述", "当时", "历史", "~~"]
# 例外：含"原文"但同时是活跃陈述？—— 保守起见，一律按历史留痕处理。

CIRC = [chr(0x2460 + i) for i in range(20)]        # ①..⑳
CIRC_MAP = {c: i + 1 for i, c in enumerate(CIRC)}

# 段标题：形如 "⑩ 段" / "第 ⑩ 段"
RE_SEG_TITLE = re.compile(r"([%s])\s*段" % "".join(CIRC))
# 只是普通段号陈述（P1 用：含"第 N 段"或"N 段"）
RE_SEG_ANY = RE_SEG_TITLE
# 剩余片陈述（P2 用）："剩余片6" / "剩余 片6–片8" / "剩余片 6"
RE_REMAIN = re.compile(r"剩余\s*片\s*[0-9%s]" % "".join(CIRC))
# 已收口标记（P3 用）——**收窄到与"迁移/分片"相关的表述**，避免把
# 无关的「D28/D29 已收口」之类误算进来（假阳性 = 判据可信度的杀手）。
RE_DONE = re.compile(r"(?:(?:迁移|片)[^。\n]{0,20}已收口|已收口[^。\n]{0,20}(?:迁移|片)|无剩余片|逐片收口)")

# ---- 模块数陈述（P4 用 · R80-1 落地 2026-09-19）----------------------------------
# 口径（R80-1/R80-3/R80-4）：**只做「模块数」一类**判据。
#   · **不做**：测试计数（10 行）／行数（21 行）／条目计数（53 行）—— 误报率高（R73 教训），
#     且「分片增量写法」如「+59＝片6」会被误伤（**R80-3 明确留痕：不做**）。
#   · **豁免**：版本锚 `v0.xxx`／`HEAD <hash>` —— 快照天然落后 1–N 个 commit，**判红必误**
#     （**R80-4 明确豁免**；并已含在 `HISTORY_MARKS` 的"当时/历史"豁免精神内）。
# 检索式**刻意收窄**为两种明确写法（源 N／目标 M ＋ 目标 crate N 模块），避免把无关数字误算。
# ★ **2026-09-19 加固（修「粗体阻断」漏报）**：原检索式不容忍 markdown 强调符 ⇒
#   `✅ PASS（源 **56** ／ 目标 **57**）` **整条漏检**（实测就漏了这一条）。
#   现插入 `_EM = \s*[*_]{0,2}\s*`，容忍 `**`/`*`/`__` 与空白 ⇒ 强调不再阻断。
_EM = r"\s*[*_]{0,2}\s*"
RE_MOD_SRC = re.compile(
    r"源" + _EM + r"(?:crate" + _EM + r")?(\d{1,3})" + _EM + r"[／/]" + _EM
    + r"目标" + _EM + r"(?:crate" + _EM + r")?(\d{1,3})")
RE_MOD_TGT = re.compile(r"目标" + _EM + r"crate" + _EM + r"(\d{1,3})" + _EM + r"模块")

# ★ 2026-09-19 加固（②）：**表格级历史豁免** —— 表头（`| … | … |` 后紧跟 `|---|` 分隔行）含
#   任一 `HISTORY_MARKS` ⇒ **整表**视为历史留痕（"修订前 / 修订后"这类对照表的前值列不再误报）。
RE_TABLE_ROW = re.compile(r"^\s*\|.*\|\s*$")
RE_TABLE_SEP = re.compile(r"^\s*\|[-\s:|]+\|\s*$")


def history_table_lines(lines):
    """返回「位于历史表内」的行号集合（1-based）。表头含 HISTORY_MARKS ⇒ 整表为历史。"""
    out = set()
    n = len(lines)
    i = 0
    while i < n:
        # 表头在第 i 行、分隔行在第 i+1 行、且第 i+1 行是表格分隔符 ⇒ 成表
        # 成表条件：第 i 行是表格行、第 i+1 行是 `|---|---|` 分隔行（分隔行本身也匹配表格行正则）
        if RE_TABLE_ROW.match(lines[i]) and i + 1 < n and RE_TABLE_SEP.match(lines[i + 1]):
            header = lines[i]
            if any(k in header for k in HISTORY_MARKS):
                out.add(i + 1)                      # 表头行本身
                out.add(i + 2)                      # 分隔行
                j = i + 2
                while j < n and RE_TABLE_ROW.match(lines[j]):
                    out.add(j + 1)
                    j += 1
                i = j
                continue
        i += 1
    return out


def _active(line: str) -> bool:
    """该行是否为**活跃**陈述（非历史留痕）。"""
    return not any(k in line for k in HISTORY_MARKS)


_HIST_LINES = set()  # 当前文件内「历史表」行号（★ 表格级历史豁免；由 check() 逐文件刷新）


def _active_at(lines, lineno: int) -> bool:
    """行号版活跃判定：**行内历史标记词** 或 **该行属于历史表** 任一成立 ⇒ 非活跃。"""
    if not _active(lines[lineno - 1]):
        return False
    return lineno not in _HIST_LINES


def circ_of(line: str):
    """取一行内出现的段号（可能多个）。"""
    return [CIRC_MAP[c] for c in RE_SEG_TITLE.findall(line)]


def scan_targets(repo: Path):
    """扫描范围：coordination 顶层 + README.md + docs/*.md（排除 reports/ discussions/）。"""
    out = []
    coord = repo / "coordination"
    if coord.is_dir():
        for f in sorted(coord.glob("*.md")):
            out.append(f)
    rd = repo / "README.md"
    if rd.is_file():
        out.append(rd)
    docs = repo / "docs"
    if docs.is_dir():
        for f in sorted(docs.glob("*.md")):
            out.append(f)
    return out


def machine_max_seg(repo: Path):
    """机器事实：verify.rs 中段标题的最大段号。返回 (maxseg, count) 或 (None, 0)。"""
    p = repo / VERIFY_REL
    if not p.is_file():
        return None, 0
    txt = p.read_text(encoding="utf-8", errors="replace")
    nums = [CIRC_MAP[c] for c in RE_SEG_TITLE.findall(txt)]
    if not nums:
        return None, 0
    return max(nums), len(nums)


def machine_is_closed(repo: Path):
    """机器事实：源模块集是否已被目标模块集覆盖（收口）。返回 (closed, src_n, tgt_n, rest)。

    ★ R79 修复（2026-09-19）：枚举**与 `check_migration_closure.py::modules()` 同源**
    （直接 import 其实现，见文件头 C18 注）。此前用 `s.glob("*.rs")`（**仅顶层**，且排除 `lib.rs`）
    ⇒ 与 closure 的**递归**口径不同 ⇒ 源 47 vs 56（差 9）。**同一事实必须只有一个数。**
    """
    s = repo / SRC_DIR
    t = repo / TGT_DIR
    if not (s.is_dir() and t.is_dir()):
        return None, 0, 0, None
    if _closure_modules is None:
        return None, 0, 0, None
    src = set(_closure_modules(s))
    tgt = set(_closure_modules(t))
    rest = sorted(src - tgt)
    return (len(rest) == 0), len(src), len(tgt), rest


def check(repo: Path, verbose=True):
    """执行三判据。返回 (errors: list[str], stats: dict)。"""
    errors = []
    stats = {}

    # ---- 锚点自检（C18 第 ①② 侧）----
    targets = scan_targets(repo)
    stats["target_files"] = len(targets)
    if len(targets) == 0:
        errors.append("[锚点] 扫描范围解析到 0 个文件 —— 判据空转（区分：这不是 PASS）")

    mx, cnt = machine_max_seg(repo)
    stats["machine_max_seg"] = mx
    stats["machine_seg_titles"] = cnt
    if mx is None:
        errors.append("[锚点] %s 未解析到任何段标题 —— 机器事实源不可读（判据空转）" % VERIFY_REL)
    elif cnt < 5:
        errors.append("[锚点] %s 仅解析到 %d 个段标题（期望 >=5）—— 疑似解析式失效" % (VERIFY_REL, cnt))

    closed, sn, tn, rest = machine_is_closed(repo)
    stats["src_modules"] = sn
    stats["tgt_modules"] = tn
    stats["rest_modules"] = rest

    # ---- 逐文件收集活跃陈述 ----
    per_file_seg = {}      # file -> [(lineno, seg)]
    per_file_remain = {}   # file -> [(lineno, text)]
    per_file_done = {}     # file -> [(lineno, text)]
    per_file_mod = {}      # file -> [(lineno, kind, value)]  （P4 用）

    for f in targets:
        try:
            lines = io.open(f, encoding="utf-8", errors="replace").read().split("\n")
        except OSError:
            continue
        rel = str(f.relative_to(repo)).replace("\\", "/")
        _HIST_LINES.clear()
        _HIST_LINES.update(history_table_lines(lines))   # ★ 表格级历史豁免（逐文件重算）
        for i, l in enumerate(lines, 1):
            if not _active_at(lines, i):
                continue
            for s in circ_of(l):
                per_file_seg.setdefault(rel, []).append((i, s))
            if RE_REMAIN.search(l):
                per_file_remain.setdefault(rel, []).append((i, l.strip()[:120]))
            if RE_DONE.search(l):
                per_file_done.setdefault(rel, []).append((i, l.strip()[:120]))
            for mm in RE_MOD_SRC.finditer(l):
                per_file_mod.setdefault(rel, []).append((i, "源", int(mm.group(1))))
                per_file_mod.setdefault(rel, []).append((i, "目标", int(mm.group(2))))
            for mm in RE_MOD_TGT.finditer(l):
                per_file_mod.setdefault(rel, []).append((i, "目标", int(mm.group(1))))

    stats["active_mod_claims"] = sum(len(v) for v in per_file_mod.values())

    stats["active_seg_files"] = {k: sorted(set(v for _, v in vals)) for k, vals in per_file_seg.items()}
    stats["active_remain"] = per_file_remain

    n_act_seg = sum(len(v) for v in per_file_seg.values())
    stats["active_seg_count"] = n_act_seg
    if n_act_seg == 0:
        errors.append("[锚点] 全库未解析到任何活跃段号陈述 —— 疑似解析式失效（判据空转）")

    # ---- P1：逐文件段号最大值须 == 机器值 ----
    if mx is not None:
        for rel, vals in sorted(per_file_seg.items()):
            fmax = max(s for _, s in vals)
            if fmax != mx:
                loc = ", ".join("行%d" % i for i, s in vals if s == fmax)
                errors.append(
                    "[P1 段号落后] %s：活跃段号最大值 = 第 %s 段（%s），"
                    "机器实测 = 第 %s 段 ⇒ 差 %d 段"
                    % (rel, CIRC[fmax - 1], loc, CIRC[mx - 1], mx - fmax))

    # ---- P2a：机器**已收口** ⇒ 不得有活跃"剩余 N 片" ----
    if closed:
        for rel, vals in sorted(per_file_remain.items()):
            for i, txt in vals:
                errors.append("[P2a 收口不符] %s 行%d 仍称「剩余 N 片」（机器已收口：源 %d / 目标 %d、剩余 0）\n      %s"
                              % (rel, i, sn, tn, txt))

    # ---- P2b（R79 双侧 · 新增）：机器**未收口** ⇒ 文档不得称"已收口" ----
    if closed is False and rest:
        for rel, vals in sorted(per_file_done.items()):
            for i, txt in vals:
                errors.append("[P2b 收口不符（反向）] %s 行%d 称「已收口」，但机器**未收口**"
                              "（源 %d / 目标 %d、剩余 %d：%s）\n      %s"
                              % (rel, i, sn, tn, len(rest), rest[:6], txt))

    # ---- P3：同文件内部自洽（既有"已收口"又有活跃"剩余 N 片"）----
    for rel in sorted(set(per_file_done) & set(per_file_remain)):
        d = per_file_done[rel][0][0]
        r = per_file_remain[rel][0][0]
        errors.append("[P3 文档内部矛盾] %s：行%d 称「已收口」，行%d 又称「剩余 N 片」" % (rel, d, r))

    # ---- P4（R80-1 落地 · 2026-09-19）：活跃「模块数」陈述须 == 机器值 ----
    # 机器值来自 `machine_is_closed()`（**内部 import `check_migration_closure.py::modules()`** ⇒ C19 同源）。
    # 口径：**只比"模块数"**；其余数值型（测试计数/行数/条目数）**不做**（R80-3）；版本锚**豁免**（R80-4）。
    #
    # ★ **首轮只提示、不判红（R60 先例）** —— 首跑实测（2026-09-19）：
    #   · **误报 3 项**：`BASELINE.md:695`（记录 R79 **前**旧口径的历史数字）／`:884`（「**修订前**」列的
    #     前值）—— 二者**语义上是历史快照**，只是行内没有历史标记词 ⇒ `_active()` 认不出。
    #   · **漏报 1 项**：`BASELINE.md:694` `✅ PASS（源 **56** ／ 目标 **57**）` —— 机器实测**目标 58**；
    #     漏报根因＝markdown `**` 粗体夹在「源/目标」与数字之间，**阻断正则**。
    #   ⇒ 判据**既误报又漏报**，**不足以判红**；依 R60 先例降为提示。
    #   ⇒ **转判红的前置（缺一不可）**：① 裁定 694／695／884 三处如何处置；
    #     ② 修掉「粗体阻断」缺陷（让正则容忍 `*`/`_` 强调符）；③ 表格级历史豁免（表头含历史标记词
    #        ⇒ 整表视为历史留痕）。**在此之前：不判红，也不放宽检索式**（不调参）。
    p4_obs = []
    for rel, vals in sorted(per_file_mod.items()):
        for i, kind, v in vals:
            want = sn if kind == "源" else tn
            if want and v != want:
                p4_obs.append(
                    "[P4 提示·待裁] %s 行%d 称「%s %d」，机器实测「%s %d」（closure 同源）"
                    % (rel, i, kind, v, kind, want))
    stats["p4_observations"] = p4_obs

    # ---- 输出 ----
    if verbose:
        print("=" * 68)
        print("机制 25 · 文档一致性判据")
        print("=" * 68)
        print("扫描范围      ：%d 份（coordination 顶层 + README + docs/*.md）" % stats["target_files"])
        print("机器·最大段号 ：%s（段标题 %d 条，来源 %s）" % (CIRC[mx - 1] if mx else "解析失败", cnt, VERIFY_REL))
        print("机器·迁移收口 ：%s（源 %d / 目标 %d / 剩余 %d）%s"
              % ("已收口" if closed else "未收口", sn, tn, len(rest) if rest else 0,
                 ("：%s" % rest) if rest else ""))
        print("枚举口径      ：与 check_migration_closure.py **同源**（递归；R79 修复，C18）")
        print("活跃段号陈述  ：%d 条" % n_act_seg)
        print("活跃模块数陈述：%d 条（P4 · R80-1）" % stats.get("active_mod_claims", 0))
        for rel, vals in sorted(per_file_seg.items()):
            print("    %-32s %s（max=第 %s 段）"
                  % (rel, [CIRC[s - 1] for s in sorted(set(v for _, v in vals))], CIRC[max(v for _, v in vals) - 1]))
        print("-" * 68)
        if errors:
            print("结果：❌ **判红**（%d 条）" % len(errors))
            for e in errors:
                print("  " + e)
        else:
            print("结果：✅ 通过（P1 段号一致 / P2 收口一致〔双侧 P2a＋P2b〕 / P3 内部自洽）")
        if stats.get("p4_observations"):
            print("⚠️  P4 模块数（**首轮提示 · 不判红**，R60 先例）：%d 条待裁"
                  % len(stats["p4_observations"]))
            for x in stats["p4_observations"]:
                print("  " + x)
            print("    （口径：只做「模块数」一类；测试计数/行数/条目数不做〔R80-3〕；版本锚豁免〔R80-4〕）")
        print("=" * 68)

    return errors, stats


# --------------------------- 自检（C18 七侧） ---------------------------
def selftest() -> int:
    print("=" * 68)
    print("机制 25 · 自检（C18：同源 ＋ 回读真实仓库 ＋ 可区分 0 与解析失败）")
    print("=" * 68)
    repo = Path(".").resolve()
    fails = []

    # 侧 ①：真实仓库锚点可解析
    t = scan_targets(repo)
    print("\n[侧①] 真实仓库锚点：扫描到 %d 份文件" % len(t))
    if len(t) < 10:
        fails.append("侧① 扫描范围过小（%d）" % len(t))
    else:
        print("      ✅ >0 且量级合理")

    # 侧 ②：机器事实源可解析
    mx, cnt = machine_max_seg(repo)
    print("[侧②] 机器事实源：%s ⇒ 段标题 %d 条、最大 = %s"
          % (VERIFY_REL, cnt, CIRC[mx - 1] if mx else "解析失败"))
    if mx is None or cnt < 5:
        fails.append("侧② 机器事实源解析失败")
    else:
        print("      ✅ 可解析")

    # 侧 ③④⑤：夹具（复用被测实现的同一解析式与常量 ⇒ 同源）
    d = Path(tempfile.mkdtemp(prefix="doc_"))
    (d / "coordination").mkdir()
    (d / "docs").mkdir()
    (d / "meta-kernel-boot" / "kernel" / "src").mkdir(parents=True)
    (d / "meta-kernel-core" / "src").mkdir(parents=True)
    (d / "meta-kernel-core-nostd" / "src").mkdir(parents=True)

    # 机器侧夹具：段号到 ⑩，且已收口
    (d / VERIFY_REL).write_text(
        "\n".join("// —— %s 段：夹具（%d）——" % (CIRC[i - 1], i) for i in range(6, 11)),
        encoding="utf-8")
    (d / SRC_DIR / "a.rs").write_text("// x", encoding="utf-8")
    (d / TGT_DIR / "a.rs").write_text("// x", encoding="utf-8")
    (d / "README.md").write_text("# 夹具\n", encoding="utf-8")
    (d / "docs" / "X.md").write_text("# x\n", encoding="utf-8")

    # 侧 ③：一致文档 ⇒ 不判红
    (d / "coordination" / "OK.md").write_text(
        "# 夹具·一致\n\n裸机断言已到第 ⑩ 段。\n3.2 已收口，无剩余片。\n", encoding="utf-8")
    e3, _ = check(d, verbose=False)
    print("\n[侧③] 夹具·一致文档 ⇒ %s" % ("❌ 误判红" if e3 else "✅ 未判红"))
    if e3:
        fails.append("侧③ 误判红：%s" % e3)

    # 侧 ④：段号落后 ⇒ 判红
    (d / "coordination" / "OK.md").write_text("# 夹具·落后\n\n裸机断言已到第 ⑧ 段。\n", encoding="utf-8")
    e4, _ = check(d, verbose=False)
    print("[侧④] 夹具·段号落后（⑧ vs 机器 ⑩）⇒ %s" % ("✅ 判红" if e4 else "❌ 漏判"))
    if not e4:
        fails.append("侧④ 漏判")
    else:
        print("      捕获：%s" % e4[0][:96])

    # 侧 ⑤：收口却说"剩余" ⇒ 判红（且命中 P2）
    (d / "coordination" / "OK.md").write_text("# 夹具·剩余\n\n裸机断言已到第 ⑩ 段。剩余片6–片8 = 7 模块。\n",
                                              encoding="utf-8")
    e5, _ = check(d, verbose=False)
    print("[侧⑤] 夹具·已收口却说「剩余片6–片8」⇒ %s" % ("✅ 判红" if e5 else "❌ 漏判"))
    if not e5:
        fails.append("侧⑤ 漏判")
    else:
        print("      捕获：%s" % e5[0][:96])

    # 侧 ⑤b（加分）：历史留痕不应被判红（防"把登记表自己判红"）
    (d / "coordination" / "OK.md").write_text(
        "# 夹具·历史留痕\n\n裸机断言已到第 ⑩ 段。\n**原文**为「剩余片6–片8」／「第 ⑧ 段」。\n",
        encoding="utf-8")
    e6, _ = check(d, verbose=False)
    print("[侧⑤b] 夹具·历史留痕（含「原文」）⇒ %s" % ("✅ 未判红" if not e6 else "❌ 误判红"))
    if e6:
        fails.append("侧⑤b 误判红：%s" % e6)

    # 侧 ⑥（R79 双侧 · 新增）：机器**未收口** 却说"已收口" ⇒ 判红（命中 P2b）
    (d / SRC_DIR / "b.rs").write_text("// 只存在于源 crate（未迁）", encoding="utf-8")
    (d / "coordination" / "OK.md").write_text(
        "# 夹具·未收口却说已收口\n\n裸机断言已到第 ⑩ 段。\n3.2 已收口，无剩余片。\n", encoding="utf-8")
    e7, s7 = check(d, verbose=False)
    hit_p2b = any("P2b" in x for x in e7)
    print("[侧⑥] 夹具·机器未收口（源多 1 个 `b.rs`）却说「已收口」⇒ %s（命中 P2b=%s）"
          % ("✅ 判红" if e7 else "❌ 漏判", "✅" if hit_p2b else "❌"))
    if not e7 or not hit_p2b:
        fails.append("侧⑥ 漏判或未命中 P2b：%s" % e7)
    else:
        print("      捕获：%s" % e7[0][:100])
    # 复原（删掉未迁模块，避免影响后续）
    os.remove(str(d / SRC_DIR / "b.rs"))

    # 侧 ⑦（R80-1 落地 · 新增）：模块数陈述 == 机器值；过期 ⇒ 判红（**双向**）
    #   夹具机器值：源 1 / 目标 1（各一个 a.rs）。
    (d / "coordination" / "OK.md").write_text(
        "# 夹具·模块数一致\n\n裸机断言已到第 ⑩ 段。\n收口：源 1 ／ 目标 1。\n", encoding="utf-8")
    e8, s8 = check(d, verbose=False)
    obs8 = s8.get("p4_observations") or []
    print("[侧⑦a] 夹具·模块数一致（源 1／目标 1）⇒ %s"
          % ("❌ 误报" if obs8 else "✅ 未误报（也无判红）"))
    if obs8 or e8:
        fails.append("侧⑦a P4 误报/误判红：obs=%s errs=%s" % (obs8, e8))
    (d / "coordination" / "OK.md").write_text(
        "# 夹具·模块数过期\n\n裸机断言已到第 ⑩ 段。\n收口：源 55 ／ 目标 57。\n", encoding="utf-8")
    e9, s9 = check(d, verbose=False)
    obs9 = s9.get("p4_observations") or []
    hit9 = any("源 55" in x for x in obs9)
    print("[侧⑦b] 夹具·模块数过期（源 55／目标 57 vs 机器 1／1）⇒ %s"
          % ("✅ 命中 P4 提示（且**不判红**）" if (hit9 and not e9) else "❌ 漏判或误判红"))
    if not hit9 or e9:
        fails.append("侧⑦b P4 未按预期提示/误判红：errs=%s obs=%s" % (e9, obs9))
    else:
        print("      捕获：%s" % obs9[0][:100])

    print("\n" + "=" * 68)
    if fails:
        print("自检结论：❌ 失败 %d 项" % len(fails))
        for x in fails:
            print("  - %s" % x)
        return 1
    print("自检结论：✅ 七侧全部符合预期")
    print("=" * 68)
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description="机制 25 · 文档一致性判据")
    ap.add_argument("--selftest", action="store_true", help="自检（七侧）")
    ap.add_argument("--list", action="store_true", help="只列活跃陈述")
    ap.add_argument("--repo", default=".", help="仓库根")
    a = ap.parse_args()
    if a.selftest:
        return selftest()
    errs, _ = check(Path(a.repo).resolve(), verbose=True)
    if a.list:
        return 0
    return 1 if errs else 0


if __name__ == "__main__":
    sys.exit(main())
