#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""机制 25 · 文档一致性判据（check_doc_consistency.py）

【为什么需要它】
R69／R72 家族：**"迁移进度"这类状态型文档会静默过期，且没有任何判据在检查它**。
R62 是"判据盲区"（判据只证明没多改字、不证明改了的字对）；本判据补的是它的**文档侧同族**：
**文档说的状态，与机器实测的状态，是否一致**。

【四条判据】
P1 · **段号一致性（文档 ↔ 机器）**：逐文件取"活跃段号陈述"的**最大值**，须等于机器实测最大段号。
     —— 口径说明（R35）：取"每文件最大值"而非"逐条比对"，因为段号是**单调递增的历史叙事**
     （文档会记录"扩到第 ⑥ 段→⑧→⑩"的过程）；**落后的是"最大值"，不是每一条**。
     —— ★ **机器段号源＝boot kernel 全部 `.rs`（递归）**（2026-09-20 缺陷③；此前只读 `verify.rs`
     ⇒ **看不见写在 `main.rs` 的 ⑫ 段**）。
P2 · **收口一致性（文档 ↔ 机器 · ★ R79 起为「双侧」）**：
     ① **P2a**：机器**已收口** ⇒ 文档不得仍有"剩余 N 片"的**活跃**陈述；
     ② **P2b**：机器**未收口** ⇒ 文档不得称"已收口"（**反向**，R79 前缺失该侧 ⇒ 单侧盲区）。
P3 · **文档内部一致性（同文件自洽）**：同一文件内既有"已收口"又有活跃"剩余 N 片"⇒ 自相矛盾。
P4 · **模块数一致性（文档 ↔ 机器 · ★ 2026-09-20 **转判红**）**：活跃「源 N ／ 目标 M」「目标 crate N 模块」
     陈述须等于机器值（closure 同源）。**只做「模块数」一类**；测试计数/行数/条目数**不做**（R80-3）；
     版本锚 `v0.xxx`／`HEAD <hash>` **豁免**（R80-4）。**实跑命中即判红**（D-2；首轮曾是"只提示"）。

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
② 机器事实源：boot kernel 全部 `.rs` 必须解析到 >=5 处段号（否则判据空转）；
③ 夹具·一致文档 ⇒ 不判红；④ 夹具·段号落后 ⇒ 判红；⑤ 夹具·收口却说剩余 ⇒ 判红；
⑥ 夹具·未收口却说已收口 ⇒ 判红（P2b 反向侧）；
⑦ 夹具·模块数过期 ⇒ **判红**（P4；⑦a 一致不误报）；⑧ 夹具·段号源多文件 ⇒ 取全体最大（缺陷③）。
⑨ **C-02** 豁免词**只减不增**（现行词表无扩张；注入反例词 ⇒ 可判红）；
⑩ **C-03** **零陈述文档 ⇒ `NO_CLAIM`**（**与 `PASS` 不同值**；对照·含陈述 ⇒ `HAS_CLAIM`）；
⑪ **C-04** **机器事实源退化 ⇒ 判红**（闭包源不可读 ⇒ `CLOSURE_EMPTY`；对照·源齐备 ⇒ 不报）；
⑫ **C-01** **多文件同时过期 ⇒ 逐份点名**（2 份 ⇒ 两份都点名；对照·1 份 ⇒ 只点名 1 份）。

【★ 2026-09-21 加固（C-02／C-03／C-04 · 用户裁定「合并为一次 P3」）】
三条同属 **R83 家族**（**"退化／跳过／空转"必须与"通过"取不同值**）：
  · **C-02「豁免词只减不增」**：豁免词是本判据的**弱化旋钮**（加一个常用词即可让一批过期陈述免检）
    ⇒ 用 `HISTORY_MARKS_BASELINE` 冻结，判据＝**子集**判定（只许减、不许增）。
  · **C-03「零陈述 ⇒ NO_CLAIM」**：文档一条活跃陈述都没有时，"**没被检查**"与"**检查通过**"
    **取同值** ⇒ 假绿；现标为 **`NO_CLAIM`**（独立取值，**不并入 errors**）。
  · **C-04「机器事实源退化 ⇒ 判红」**：段号源空／闭包源不可读时，P1–P4 会**静默跳过**
    ⇒ 与"通过"同值；现**判红**并打 `SEG_EMPTY`／`CLOSURE_EMPTY` 标记。
  · **范围**：只改**本判据脚本**；**未改任何治理文件**（`TEMPLATES.md` 等一律未动）。

【★ 口径同源（C18 · R79 修复）】
本判据的"机器事实·迁移收口"**直接 import** `check_migration_closure.py::modules()`，
**不另写第二份枚举**。此前用 `glob("*.rs")`（**仅顶层**）⇒ 与 closure 的递归口径差 **9 个模块**
（源 47 vs 56：漏 `l4/*`＋`l7/*` 共 8 个 + `lib` 归一差异），且**同一事实两个数** ⇒ 二义性。

用法：
    python coordination/tools/check_doc_consistency.py            # 实跑
    python coordination/tools/check_doc_consistency.py --selftest # 八侧自检
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
# ★ 2026-09-20 · **缺陷③ 修复**：段号源**不再写死 `verify.rs`**。
#   根因：`present` 的 **⑫ 段接线写在 `main.rs`**（`run()` 里）⇒ 旧口径的"机器最大段号"= **⑪**，
#   而机器**真实**最大 = **⑫** ⇒ 判据**看不见 ⑫ 段**（R80 家族：判据覆盖盲区）。
#   修法：扫 **boot kernel 的全部 `.rs`（递归）**，取全体最大段号 —— **不写死文件名**，
#   将来新增 boot 文件（如 `present2.rs`）自动纳入，不再重演"新增文件被漏"。
BOOT_KERNEL_SRC = "meta-kernel-boot/kernel/src"
SRC_DIR = "meta-kernel-core/src"
TGT_DIR = "meta-kernel-core-nostd/src"

# 历史标记词：含这些词的行 = 历史留痕，不参与判据
HISTORY_MARKS = ["修订前", "改前", "原为", "原文", "已处置", "已修订", "订正",
                 "引述", "当时", "历史", "~~"]
# 例外：含"原文"但同时是活跃陈述？—— 保守起见，一律按历史留痕处理。

# ★ **C-02（2026-09-21 加固）· 豁免词「只减不增」**
#   病根：豁免词是本判据的**弱化旋钮** —— 往里加一个常用词（如「计划」）即可让**一批过期陈述
#   静默免检**（≡ 弱化判据，与 C13「弱化判据即为破戒」同族）。
#   口径：**允许删（更严）、禁止增** —— 判据＝`set(HISTORY_MARKS) ⊆ HISTORY_MARKS_BASELINE`。
#   **本基线一旦锁定，只许缩小、不许扩大**；要扩大须**明示裁定**并同步改本常量与自检侧。
HISTORY_MARKS_BASELINE = frozenset(["修订前", "改前", "原为", "原文", "已处置", "已修订",
                                    "订正", "引述", "当时", "历史", "~~"])


def history_marks_expansion(marks):
    """C-02 纯函数：返回**超出基线**的豁免词（空 ⇒ 合规）。便于自检注入对照。"""
    return sorted(set(marks) - set(HISTORY_MARKS_BASELINE))

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


def boot_seg_sources(repo: Path):
    """机器事实源：**boot kernel 的全部 `.rs`（递归）**。**不写死文件名**（缺陷③）。

    为什么必须递归且不写死：段号叙事会**分散在不同文件**（`verify.rs` 是主体，但
    `main.rs` 挂 ⑫ 段接线、`mem/*.rs` 可能自带段号）⇒ 写死单文件必然漏（这就是缺陷③）。
    """
    root = repo / BOOT_KERNEL_SRC
    if not root.is_dir():
        return []
    return sorted(root.glob("**/*.rs"))


def machine_max_seg(repo: Path):
    """机器事实：boot kernel **全部 `.rs`** 中段号的最大值。

    返回 `(maxseg, count, per_file)`；`per_file = [(rel, maxseg, n), ...]`（只含解析到段号的文件）。
    无来源 / 无段号 ⇒ `(None, 0, [])`。

    ★ **2026-09-20（缺陷③）**：口径从「**只读 `verify.rs`**」扩为「**boot kernel 全部 `.rs`（递归）**」。
    实测差异：旧口径 = **⑪**（漏 `main.rs` 的 ⑫ 段接线）；新口径 = **⑫**。
    """
    files = boot_seg_sources(repo)
    if not files:
        return None, 0, []
    mx = None
    cnt = 0
    per_file = []
    for f in files:
        try:
            txt = f.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        nums = [CIRC_MAP[c] for c in RE_SEG_TITLE.findall(txt)]
        if not nums:
            continue
        rel = str(f.relative_to(repo)).replace("\\", "/")
        per_file.append((rel, max(nums), len(nums)))
        cnt += len(nums)
        mx = max(nums) if mx is None else max(mx, max(nums))
    if mx is None:
        return None, 0, []
    return mx, cnt, per_file


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

    # ---- ★ C-02（2026-09-21）：豁免词「只减不增」----
    extra_marks = history_marks_expansion(HISTORY_MARKS)
    stats["history_marks_extra"] = extra_marks
    stats["history_marks_n"] = len(HISTORY_MARKS)
    if extra_marks:
        errors.append("[C-02 豁免词扩张] 历史豁免词**新增** %s ⇒ 判据被**静默弱化**"
                      "（口径：只许减、不许增）" % extra_marks)

    # ---- 锚点自检（C18 第 ①② 侧）----
    targets = scan_targets(repo)
    stats["target_files"] = len(targets)
    if len(targets) == 0:
        errors.append("[锚点] 扫描范围解析到 0 个文件 —— 判据空转（区分：这不是 PASS）")

    mx, cnt, seg_srcs = machine_max_seg(repo)
    stats["machine_max_seg"] = mx
    stats["machine_seg_titles"] = cnt
    stats["machine_seg_sources"] = seg_srcs
    src_empty = []
    if mx is None:
        # ★ C-04（2026-09-21）：**退化必须与「通过」取不同值**（R83／R68 同族）
        errors.append("[C-04 退化·段号源空] %s 下未解析到任何段号 —— 机器事实源不可读；"
                      "本态**不得与「通过」同值**（判据空转 ⇒ 假绿）" % BOOT_KERNEL_SRC)
        src_empty.append("SEG_EMPTY")
    elif cnt < 5:
        errors.append("[锚点] %s 下仅解析到 %d 处段号（期望 >=5）—— 疑似解析式失效" % (BOOT_KERNEL_SRC, cnt))
        src_empty.append("SEG_THIN")

    closed, sn, tn, rest = machine_is_closed(repo)
    stats["src_modules"] = sn
    stats["tgt_modules"] = tn
    stats["rest_modules"] = rest
    if closed is None:
        # ★ C-04：闭包源不可读时，P2a／P2b／P4 会**静默跳过** ⇒ 与「通过」同值（R83 病象）
        errors.append("[C-04 退化·闭包源不可读] 源/目标 crate 或其枚举不可用 ⇒ P2a／P2b／P4 将**静默跳过**；"
                      "本态**不得与「通过」同值**（判据空转 ⇒ 假绿）")
        src_empty.append("CLOSURE_EMPTY")
    stats["source_empty"] = src_empty

    # ---- 逐文件收集活跃陈述 ----
    per_file_seg = {}      # file -> [(lineno, seg)]
    per_file_remain = {}   # file -> [(lineno, text)]
    per_file_done = {}     # file -> [(lineno, text)]
    per_file_mod = {}      # file -> [(lineno, kind, value)]  （P4 用）
    rel_claims = {}        # file -> 活跃断言**条数**（★ C-03：0 ⇒ NO_CLAIM，与 PASS 不同值）

    for f in targets:
        rel = str(f.relative_to(repo)).replace("\\", "/")
        rel_claims.setdefault(rel, 0)
        try:
            lines = io.open(f, encoding="utf-8", errors="replace").read().split("\n")
        except OSError:
            continue                      # 读不到 ⇒ 保持 0（C-03 会把它标为 NO_CLAIM）
        _HIST_LINES.clear()
        _HIST_LINES.update(history_table_lines(lines))   # ★ 表格级历史豁免（逐文件重算）
        c = 0
        for i, l in enumerate(lines, 1):
            if not _active_at(lines, i):
                continue
            for s in circ_of(l):
                per_file_seg.setdefault(rel, []).append((i, s))
                c += 1
            if RE_REMAIN.search(l):
                per_file_remain.setdefault(rel, []).append((i, l.strip()[:120]))
                c += 1
            if RE_DONE.search(l):
                per_file_done.setdefault(rel, []).append((i, l.strip()[:120]))
                c += 1
            n_mod = 0
            for mm in RE_MOD_SRC.finditer(l):
                per_file_mod.setdefault(rel, []).append((i, "源", int(mm.group(1))))
                per_file_mod.setdefault(rel, []).append((i, "目标", int(mm.group(2))))
                n_mod += 2
            for mm in RE_MOD_TGT.finditer(l):
                per_file_mod.setdefault(rel, []).append((i, "目标", int(mm.group(1))))
                n_mod += 1
            c += n_mod
        rel_claims[rel] = rel_claims.get(rel, 0) + c

    # ---- ★ C-03（2026-09-21）：**零陈述文档 ⇒ NO_CLAIM**（独立取值，≠ PASS）----
    #   病根（R83 同族）：文档一条活跃陈述都没有时，P1/P2/P3/P4 **全部无事可做** ⇒
    #   「**该文档根本没被检查**」与「**该文档检查通过**」**取同值**（假绿）。
    #   处置：把它标成 **NO_CLAIM**（**与 PASS 不同值**），在 stats 与输出里**单列**。
    #   ⚠️ 本项**不改判红策略**（NO_CLAIM 不并入 errors）—— 它只负责"**可区分**"。
    no_claim = sorted(rel for rel, c in rel_claims.items() if c == 0)
    stats["no_claim_files"] = no_claim
    stats["verdicts"] = {rel: ("NO_CLAIM" if c == 0 else "HAS_CLAIM")
                         for rel, c in sorted(rel_claims.items())}

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

    # ---- P4（R80-1 落地 2026-09-19 · ★ 2026-09-20 D-2 转判红）：活跃「模块数」陈述须 == 机器值 ----
    # 机器值来自 `machine_is_closed()`（**内部 import `check_migration_closure.py::modules()`** ⇒ C19 同源）。
    # 口径：**只比"模块数"**；其余数值型（测试计数/行数/条目数）**不做**（R80-3）；版本锚**豁免**（R80-4）。
    #
    # ★ **历史留痕（R60 先例期）** —— 首跑实测（2026-09-19）判据**既误报又漏报**，故首轮只提示：
    #   · 误报 3 项：`BASELINE.md:695`／`:884`（语义上是历史快照，只是行内无历史标记词）；
    #   · 漏报 1 项：`BASELINE.md:694`（markdown `**` 粗体夹在「源/目标」与数字之间 ⇒ **阻断正则**）。
    #   ⇒ 依 R60 先例降为**提示**，直到三项前置（处置／修粗体阻断／表格级豁免）**全部**完成。
    #
    # ★ **2026-09-20 · D-2：P4 转判红**（三项前置**均已完成**）—— 实跑命中即并入 `errors`。

    p4_obs = []
    for rel, vals in sorted(per_file_mod.items()):
        for i, kind, v in vals:
            want = sn if kind == "源" else tn
            if want and v != want:
                p4_obs.append(
                    "[P4 模块数不符] %s 行%d 称「%s %d」，机器实测「%s %d」（closure 同源）"
                    % (rel, i, kind, v, kind, want))
    stats["p4_observations"] = p4_obs
    # ★ **2026-09-20 · D-2：P4 已转判红。**
    #   三项前置（缺一不可）**均已完成**：① 裁定并处置 `BASELINE` 694／695／884（改「引路径＋校验命令」）；
    #   ② 修掉「粗体阻断」漏报（正则容忍 `*`/`_` 强调符）；③ 新增**表格级历史豁免**（表头含历史词 ⇒ 整表豁免）。
    #   ⇒ 实跑命中即**并入 errors** ⇒ `main()` 返回 1（判红）。**不再降级为提示。**
    errors.extend(p4_obs)

    # ---- 观测面（★ 2026-09-20 夜间加固）：**混合行**（同行 ＝ 历史标记词 ＋ 段号/模块数/剩余断言）----
    #   已知盲区（R82／R83 家族）：历史豁免是**行级/表级**的 ⇒ 若"活跃断言"与"历史词"**同一行**，
    #   该行被整行豁免 ⇒ 断言若已过期，P1／P4 **都不会判红**（实测例：`ROADMAP.md:59` 同行含
    #   「历史快照」与活跃段号陈述）。**本项不改判红策略**，只把它计入 `stats` ⇒
    #   **把"静默盲区"变成"可数的盲区"**（R71：不写死数字，留给后续判据／人工消费）。
    mixed = []
    for f in targets:
        try:
            lines = io.open(f, encoding="utf-8", errors="replace").read().split("\n")
        except OSError:
            continue
        rel = str(f.relative_to(repo)).replace("\\", "/")
        for i, l in enumerate(lines, 1):
            if _active(l):
                continue            # 活跃行不属盲区（本判据的正常管辖范围）
            if circ_of(l) or RE_MOD_SRC.search(l) or RE_MOD_TGT.search(l) or RE_REMAIN.search(l):
                mixed.append((rel, i, l.strip()[:120]))
    stats["mixed_history_lines"] = mixed

    # ---- 输出 ----
    if verbose:
        print("=" * 68)
        print("机制 25 · 文档一致性判据")
        print("=" * 68)
        print("扫描范围      ：%d 份（coordination 顶层 + README + docs/*.md）" % stats["target_files"])
        print("豁免词（C-02） ：%d 条（基线 %d，**只减不增**）｜超出基线的新增：%s"
              % (stats.get("history_marks_n", 0), len(HISTORY_MARKS_BASELINE),
                 stats.get("history_marks_extra") or "无"))
        if stats.get("source_empty"):
            print("退化标记（C-04）：%s ⇒ **本态与「通过」不同值**" % stats["source_empty"])
        print("机器·最大段号 ：%s（段号 %d 处，来源 %s 下 %d 份 .rs）"
              % (CIRC[mx - 1] if mx else "解析失败", cnt, BOOT_KERNEL_SRC, len(seg_srcs)))
        for rel, fm, n in seg_srcs:
            print("      · %-46s max=第 %s 段（%d 处）" % (rel, CIRC[fm - 1], n))
        print("机器·迁移收口 ：%s（源 %d / 目标 %d / 剩余 %d）%s"
              % ("已收口" if closed else "未收口", sn, tn, len(rest) if rest else 0,
                 ("：%s" % rest) if rest else ""))
        print("枚举口径      ：与 check_migration_closure.py **同源**（递归；R79 修复，C18）")
        print("活跃段号陈述  ：%d 条" % n_act_seg)
        print("活跃模块数陈述：%d 条（P4 · R80-1）" % stats.get("active_mod_claims", 0))
        # ★ 2026-09-20 夜间加固：**观测项**（相邻"历史词同行含断言"的盲区，见 check() 内注）
        _mix = stats.get("mixed_history_lines") or []
        print("混合行（历史词同行含段号/模块数/剩余断言）：%d 行"
              "（★ 观测项：**不改判红策略**；已知盲区，现改为可数）" % len(_mix))
        for rel, i, t in _mix[:8]:
            print("    · %s 行%d  %s" % (rel, i, t))
        if len(_mix) > 8:
            print("    · …（其余 %d 行略）" % (len(_mix) - 8))
        _nc = stats.get("no_claim_files") or []
        print("NO_CLAIM 文档（C-03 · 零活跃陈述 ⇒ **与 PASS 不同值**）：%d 份" % len(_nc))
        for rel in _nc[:8]:
            print("    · %s" % rel)
        if len(_nc) > 8:
            print("    · …（其余 %d 份略）" % (len(_nc) - 8))
        for rel, vals in sorted(per_file_seg.items()):
            print("    %-32s %s（max=第 %s 段）"
                  % (rel, [CIRC[s - 1] for s in sorted(set(v for _, v in vals))], CIRC[max(v for _, v in vals) - 1]))
        print("-" * 68)
        if errors:
            print("结果：❌ **判红**（%d 条）" % len(errors))
            for e in errors:
                print("  " + e)
        else:
            print("结果：✅ 通过（P1 段号一致 / P2 收口一致〔双侧 P2a＋P2b〕 / P3 内部自洽 / P4 模块数一致）")
        if stats.get("p4_observations"):
            print("❌ P4 模块数（**已转判红** · 2026-09-20 · D-2）：%d 条"
                  % len(stats["p4_observations"]))
            for x in stats["p4_observations"]:
                print("  " + x)
            print("    （口径：只做「模块数」一类；测试计数/行数/条目数不做〔R80-3〕；版本锚豁免〔R80-4〕）")
        print("=" * 68)

    return errors, stats


# --------------------------- 自检（C18 八侧；含 ⑤b／⑦a／⑦b／④b 边界子项） ---------------------------
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

    # 侧 ②：机器事实源可解析（★ 缺陷③：口径＝boot kernel 全部 .rs，不再只看 verify.rs）
    mx, cnt, seg_srcs = machine_max_seg(repo)
    print("[侧②] 机器事实源：%s 下 %d 份 .rs ⇒ 段号 %d 处、最大 = %s"
          % (BOOT_KERNEL_SRC, len(seg_srcs), cnt, CIRC[mx - 1] if mx else "解析失败"))
    for rel, fm, n in seg_srcs:
        print("      · %-46s max=第 %s 段" % (rel, CIRC[fm - 1]))
    if mx is None or cnt < 5:
        fails.append("侧② 机器事实源解析失败")
    else:
        print("      ✅ 可解析")

    # 侧 ③④⑤：夹具（复用被测实现的同一解析式与常量 ⇒ 同源）
    d = Path(tempfile.mkdtemp(prefix="doc_"))
    (d / "coordination").mkdir()
    (d / "docs").mkdir()
    (d / BOOT_KERNEL_SRC).mkdir(parents=True)
    (d / "meta-kernel-core" / "src").mkdir(parents=True)
    (d / "meta-kernel-core-nostd" / "src").mkdir(parents=True)

    # 机器侧夹具：段号到 ⑩，且已收口
    (d / BOOT_KERNEL_SRC / "verify.rs").write_text(
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
    hit9_err = any("P4" in x for x in e9)
    print("[侧⑦b] 夹具·模块数过期（源 55／目标 57 vs 机器 1／1）⇒ %s"
          % ("✅ 命中 P4 且**已判红**" if (hit9 and hit9_err) else "❌ 漏判或未判红"))
    if not (hit9 and hit9_err):
        fails.append("侧⑦b P4 未按预期判红：errs=%s obs=%s" % (e9, obs9))
    else:
        print("      捕获：%s" % e9[0][:100])

    # 侧 ⑧（★ 2026-09-20 · **缺陷③** · 新增）：**段号源必须是多文件**。
    #   夹具 `verify.rs` 只到 ⑩；**只在 `main.rs` 写 ⑫ 段** ⇒ `machine_max_seg` 也必须取到 ⑫。
    #   为什么必须有这一侧：缺陷③ 的病根就是"只读 `verify.rs`"⇒ 若无此侧，改回单文件**不会判红**。
    (d / BOOT_KERNEL_SRC / "main.rs").write_text(
        "// —— ⑫ 段：接线（夹具）——\n", encoding="utf-8")
    mx8, cnt8, srcs8 = machine_max_seg(d)
    ok8 = (mx8 == 12) and any(r.endswith("main.rs") for r, _, _ in srcs8)
    print("[侧⑧] 段号源多文件（`verify.rs`→⑩、`main.rs`→⑫）⇒ %s（机器最大 = %s，来源 %d 份）"
          % ("✅ 取到 ⑫（不再只看 verify.rs）" if ok8 else "❌ 仍只看 verify.rs",
             CIRC[mx8 - 1] if mx8 else "解析失败", len(srcs8)))
    if not ok8:
        fails.append("侧⑧ 段号源未覆盖 boot 其它文件：mx=%s srcs=%s" % (mx8, srcs8))
    else:
        print("      来源：%s" % ", ".join(r for r, _, _ in srcs8))
    os.remove(str(d / BOOT_KERNEL_SRC / "main.rs"))

    # 侧 ④b（★ 2026-09-20 夜间加固 · **边界组**）：**历史豁免不得"外溢"**。
    #   病根（已知盲区）：豁免是**行级**（`_active`）＋**表级**（`history_table_lines`）的 ——
    #   若实现被改成"文件级"或"直到空行/表末"，会**静默放过**相邻的活跃过期断言
    #   （**R83 家族**：漏报与"通过"取同值 ⇒ 判据静默变绿）。本侧三小项：
    #     (a) 行内历史词 ⇒ **只豁免本行**；
    #     (b) 表头历史词 ⇒ **只豁免该表**，表外正文仍须判红；
    #     (c) **观测**：同行既有历史词又有断言 ⇒ 计入 `mixed_history_lines`（把静默盲区变可数）。
    #   —— (a)(b) 是**正反对照**；(c) **不改判红策略**（已知盲区只登记、不判红）。

    # (a) 行内历史词 ⇒ 只豁免本行（下一行的活跃过期断言仍须判红）
    (d / "coordination" / "OK.md").write_text(
        "# 夹具·边界(a)\n\n**原文**为「第 ⑫ 段」。\n裸机断言已到第 ⑧ 段。\n", encoding="utf-8")
    e10, _ = check(d, verbose=False)
    hit_a = any("P1" in x for x in e10)
    print("[侧④b-a] 边界·行级豁免不外溢（第 3 行含「原文」被豁免、第 4 行活跃陈旧仍须判红）⇒ %s"
          % ("✅ 判红且命中 P1" if hit_a else "❌ 漏判（豁免外溢）"))
    if not hit_a:
        fails.append("侧④b-a 行级豁免外溢：%s" % e10)
    else:
        print("      捕获：%s" % e10[0][:96])

    # (b) 表头历史词 ⇒ 只豁免该表（表外正文的活跃「剩余」仍须判红）
    (d / "coordination" / "OK.md").write_text(
        "# 夹具·边界(b)\n\n"
        "| 项 | 历史值 |\n|---|---|\n| 片6 | 剩余片6–片8 |\n\n"
        "裸机断言已到第 ⑩ 段。剩余片6–片8 = 7 模块。\n", encoding="utf-8")
    e11, _ = check(d, verbose=False)
    hit_b = any("P2a" in x for x in e11)
    print("[侧④b-b] 边界·表级豁免不外溢（表内「剩余」豁免、表外活跃「剩余」须判红）⇒ %s"
          % ("✅ 判红且命中 P2a" if hit_b else "❌ 漏判（表级外溢）"))
    if not hit_b:
        fails.append("侧④b-b 表级豁免外溢：%s" % e11)
    else:
        print("      捕获：%s" % e11[0][:96])

    # (c) 观测：混合行可数（**不改判红策略**）
    (d / "coordination" / "OK.md").write_text(
        "# 夹具·边界(c)\n\n裸机断言已到第 ⑩ 段。\n"
        "另有一处混合行：裸机断言已到第 ⑧ 段（**原文**如此登记）。\n", encoding="utf-8")
    e12, s12 = check(d, verbose=False)
    mix = s12.get("mixed_history_lines") or []
    ok_c = len(mix) >= 1
    print("[侧④b-c] 观测·混合行（同行含历史词 ＋ 段号）可数 ⇒ %s（计 %d 行）"
          % ("✅ 可观测" if ok_c else "❌ 不可观测", len(mix)))
    if not ok_c:
        fails.append("侧④b-c 混合行不可观测：%s" % s12)
    else:
        print("      ⚠️ **本项只观测、不判红**：混合行仍按「历史留痕」豁免 ——")
        print("         这是**已知盲区**（R82／R83 家族），现改为**可数**；是否判红留待裁定。")

    # 侧 ⑨（★ 2026-09-21 加固 · **C-02**）：豁免词**只减不增**（正反对照）
    exp_now = history_marks_expansion(HISTORY_MARKS)
    exp_bad = history_marks_expansion(list(HISTORY_MARKS) + ["计划"])
    ok9 = (exp_now == []) and ("计划" in exp_bad)
    print("[侧⑨] C-02 豁免词（a）现行词表 ⇒ %s；（b）注入反例词 ⇒ %s"
          % ("✅ 无扩张（合规）" if exp_now == [] else "❌ 已扩张：%s" % exp_now,
             "✅ 可判红" if "计划" in exp_bad else "❌ 未检出"))
    if not ok9:
        fails.append("侧⑨ C-02 未按预期：now=%s bad=%s" % (exp_now, exp_bad))

    # 侧 ⑩（★ C-03）：**零陈述文档 ⇒ NO_CLAIM**（**与 PASS 不同值**）＋ 对照·含陈述 ⇒ HAS_CLAIM
    (d / "coordination" / "OK.md").write_text(
        "# 夹具·零陈述\n\n这是一段**没有任何**段号／模块数／剩余断言的正文。\n", encoding="utf-8")
    _e13, s13 = check(d, verbose=False)
    nc13 = s13.get("no_claim_files") or []
    v13 = s13.get("verdicts") or {}
    ok10a = any(r.endswith("OK.md") for r in nc13) and v13.get("coordination/OK.md") == "NO_CLAIM"
    (d / "coordination" / "OK.md").write_text(
        "# 夹具·有陈述\n\n裸机断言已到第 ⑩ 段。\n", encoding="utf-8")
    _e14, s14 = check(d, verbose=False)
    v14 = s14.get("verdicts") or {}
    ok10b = v14.get("coordination/OK.md") == "HAS_CLAIM"
    print("[侧⑩] C-03 零陈述 ⇒ %s（**NO_CLAIM ≠ PASS**）；对照·含陈述 ⇒ %s"
          % ("✅ 标为 NO_CLAIM" if ok10a else "❌ 未标 NO_CLAIM",
             "✅ 标为 HAS_CLAIM" if ok10b else "❌ 仍标 NO_CLAIM"))
    if not (ok10a and ok10b):
        fails.append("侧⑩ C-03 未按预期：nc=%s v13=%s v14=%s" % (nc13, v13, v14))

    # 侧 ⑪（★ C-04）：**机器事实源退化 ⇒ 判红**（不得与「通过」同值；R83／R68 同族）
    _tgt = d / TGT_DIR
    _bak = d / (TGT_DIR + "__bak")
    _tgt.rename(_bak)                                   # 令闭包源不可读
    e15, s15 = check(d, verbose=False)
    ok11a = any("C-04" in x for x in e15) and ("CLOSURE_EMPTY" in (s15.get("source_empty") or []))
    _bak.rename(_tgt)                                   # 复原
    e16, _s16 = check(d, verbose=False)
    ok11b = not any("C-04" in x for x in e16)
    print("[侧⑪] C-04 退化（a）闭包源不可读 ⇒ %s；（b）对照·源齐备 ⇒ %s"
          % ("✅ 判红且标 CLOSURE_EMPTY" if ok11a else "❌ 未判红（假绿）",
             "✅ 不报 C-04" if ok11b else "❌ 误报"))
    if not (ok11a and ok11b):
        fails.append("侧⑪ C-04 未按预期：e15=%s empty=%s e16=%s"
                     % (e15, s15.get("source_empty"), e16))

    # 侧 ⑫（★ 2026-09-21 · **C-01**）：**多文件同时过期 ⇒ 逐份点名**。
    #   病根（R83 同族）：若实现里存在"命中即 break／只取第一份"，多文件过期会**只报一份** ⇒
    #   "部分生效"与"全生效"**取同值**（其余静默漏报）。本侧**正反对照**：
    #     (a) 2 份同时过期 ⇒ 必须**两份都点名**；(b) 对照·只 1 份过期 ⇒ **只点名 1 份**。
    (d / "coordination" / "A.md").write_text("# 夹具A\n\n裸机断言已到第 ⑧ 段。\n", encoding="utf-8")
    (d / "coordination" / "B.md").write_text("# 夹具B\n\n裸机断言已到第 ⑨ 段。\n", encoding="utf-8")
    _ok = d / "coordination" / "OK.md"
    _ok_bak = d / "coordination" / "OK__bak.md"
    _ok.rename(_ok_bak)                       # 暂移"第 ⑩ 段"夹具，避免它把最大段号抬到 ⑩
    _e17, _s17 = check(d, verbose=False)
    _p1 = [x for x in _e17 if "[P1" in x]
    named = set()
    for _x in _p1:
        for _fn in ("A.md", "B.md"):
            if _fn in _x:
                named.add(_fn)
    ok12a = (named == {"A.md", "B.md"})
    (d / "coordination" / "B.md").unlink()    # 阴性对照：只留 1 份过期
    _e18, _s18 = check(d, verbose=False)
    _p1b = [x for x in _e18 if "[P1" in x]
    ok12b = (len(_p1b) == 1) and ("A.md" in _p1b[0])
    _ok_bak.rename(_ok)                       # 复原
    (d / "coordination" / "A.md").unlink()
    print("[侧⑫] C-01 多文件同时过期（a）2 份 ⇒ %s；（b）对照·1 份 ⇒ %s"
          % ("✅ 逐份点名 %s" % sorted(named) if ok12a else "❌ 漏点名：%s" % sorted(named),
             "✅ 只点名 1 份" if ok12b else "❌ 异常：%s" % [x[:60] for x in _p1b]))
    if not (ok12a and ok12b):
        fails.append("侧⑫ C-01 未按预期：named=%s p1b=%s" % (sorted(named), _p1b))

    print("\n" + "=" * 68)
    if fails:
        print("自检结论：❌ 失败 %d 项" % len(fails))
        for x in fails:
            print("  - %s" % x)
        return 1
    print("自检结论：✅ 十二侧全部符合预期")
    print("=" * 68)
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description="机制 25 · 文档一致性判据")
    ap.add_argument("--selftest", action="store_true", help="自检（十二侧）")
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
