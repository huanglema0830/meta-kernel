#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""机制 17（拆分机制）· 指针检查器 —— **让"指针"可被机器校验**

为什么需要它：机制 17 规定"公开仓库只写骨架、实现层在仓库外、用 `{PRIVATE_ASSETS}/<路径>` 引用"。
**光有条文不够** —— 条文会退化成"[占位]了事"。本脚本把条文变成**可判定的检查**。

## 两个模式（**刻意分离**）

| 模式 | 在哪跑 | 检查什么 | 为什么不合并 |
|---|---|---|---|
| `--mode=syntax` | **CI**（+ 本机） | ① 每个 `{PRIVATE_ASSETS}/<路径>` **格式合法** ② 仓库内**不得新增 ⛔ 泄漏** | CI **看不到**私有文件夹（它在仓库外、且 CI 上根本不存在）⇒ 存在性检查放 CI 必然失败 |
| `--mode=resolve` | **仅本机** | ③ 每个引用目标**真实存在**（带「（待建）」标注的除外）④ 私有根**确在仓库外**（结构性隔离） | 需要私有文件夹在场 |

> **⚠️ 输出纪律（C12）**：两种模式都**只打印文件与行号，不打印命中内容** ——
> 否则"检查泄漏"的脚本自己会把泄漏内容打进 CI 日志（公开可见）。

用法：
    python3 coordination/tools/check_private_pointers.py --mode=syntax
    python3 coordination/tools/check_private_pointers.py --mode=resolve [--private-root <路径>]
    python3 coordination/tools/check_private_pointers.py --selftest      # 判据自检（十一侧）

> **2026-09-18 夜间 P3 补齐说明**：本文件曾是 `coordination/tools/` 六个判据里
> **唯一没有 `--selftest`** 的那个，而它**已被 CI 当硬门禁用**（`ci.yml` 步骤「机制 17 指针门禁」）
> ⇒ 它是唯一"**没验过自己**"的门禁。R64 的教训是「**自检与实跑口径必须同源**」：
> 没有正反对照的绿，只是"跑完了"，**不是"判对了"**。故补 `selftest()`（十一侧）。

退出码：0 = 通过；1 = 不合规（有明细输出）。
"""
from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys
from pathlib import Path

# ---------- 扫描范围 ----------

TEXT_SUFFIXES = {".md", ".rs", ".toml", ".yml", ".yaml", ".txt", ".py", ".sh", ".json", ".html"}
SKIP_DIRS = {".git", "target", "node_modules", ".workbuddy", "dist", "build"}

# ---------- 指针引用 ----------

PTR_RE = re.compile(r"\{PRIVATE_ASSETS\}([/\\][^\s]*)?")
# ⚠️ 尾类里**必须同时含 `/` 与 `\`**：初版只写 `/`，于是 `{PRIVATE_ASSETS}\x.md`
#    这种**反斜杠写法**因"后面不是斜杠"被当成"指代私有根整体"而**漏报**（本题自检发现，已修）。
# ⚠️ **尾类要极简**：初版还排除了全角标点与反引号，结果在实测中**字符类失效**（只吃到 1 个字符），
#    导致 `{PRIVATE_ASSETS}/../bad.md` 的 `..` 判不出来 —— 判据**越花越容易坏**，改回最简 `[^\s]*`。
# 「已显式标注」的豁免标记：机制 17 §10.4.3 要求"目标必须存在，尚未建立的须显式标注"。
# 这里把三类**非真实引用**也纳入豁免，但**必须显式写出标记**——
#   待建 ＝ 尚未建立；示例 ＝ 文档里的举例；模式 ＝ glob/批量（如 `0N_*_impl.md` 指 8 个文件）。
# ⚠️ 判据要求"**写了标记才算合规**"，是为了防止"悄悄加个豁免词就把门禁绕过去"。
PENDING_MARKS = ("待建", "示例", "模式")

# ---------- 泄漏模式（⛔ 类；只数个数，不回显内容） ----------
# 说明：用户名模式在**运行时**由本机 home 目录名推出，**不写死在脚本里**（写死＝脚本自身成为泄漏）。
LEAK_PATTERNS: list[tuple[str, re.Pattern[str]]] = [
    # 盘符路径：**要求 `Users/` 或 `用户/` 之后紧跟"名字型字符"**（字母/数字/下划线/汉字）。
    # —— 于是 `C:\Users\...`、`C:\Users\<用户名>` 这类**掩码示例不命中**，
    #    只有 `C:\Users\<用户名>\...` 这种**真名**才命中。
    # ⚠️ 初版写成"`Users/` 后跟非空白 2 字符"是**过宽**的：`...` 与后面的汉字之间能"跨过"
    #    反引号等标点连成一段，导致**掩码示例被误报**（本题第 2 次改判据，R16 同族教训）。
    ("win_abs_path", re.compile(
        r"\b[A-Za-z]:[\\/](?:Users|用户)[\\/](?!\.)[A-Za-z0-9_\u4e00-\u9fff][^\s\\/*?\"<>|]*"
    )),
    # 类 unix 家目录：同样要求具体段（`/c/Users/<乱码>/` 这类**掩码**不得命中）
    ("unix_home_path", re.compile(
        r"/(?:c|C|mnt/[a-z])/Users/[A-Za-z0-9_\u4e00-\u9fff]{2,}"
        r"|/(?:home|Users)/[A-Za-z0-9_\u4e00-\u9fff]{2,}/"
    )),
    ("lan_ip", re.compile(r"\b(?:192\.168|10\.\d{1,3}|172\.(?:1[6-9]|2\d|3[01]))\.\d{1,3}\.\d{1,3}\b")),
]


def _username_pattern() -> tuple[str, re.Pattern[str]] | None:
    """由本机 home 目录名推出用户名模式（**不写死**，避免脚本自身泄漏）。"""
    try:
        name = Path.home().name
    except Exception:  # pragma: no cover
        return None
    # 排除 CI 上的通用名，避免噪声
    if not name or name.lower() in {"runner", "user", "root", "administrator", "home"}:
        return None
    return ("username_literal", re.compile(re.escape(name)))


def repo_root() -> Path:
    out = subprocess.run(
        ["git", "rev-parse", "--show-toplevel"], capture_output=True, text=True, check=True
    ).stdout.strip()
    return Path(out)


def iter_files(root: Path):
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames[:] = [d for d in dirnames if d not in SKIP_DIRS]
        for fn in filenames:
            p = Path(dirpath) / fn
            if p.suffix.lower() in TEXT_SUFFIXES:
                yield p


def rel(root: Path, p: Path) -> str:
    return p.relative_to(root).as_posix()


# ---------- ① 指针格式 ----------

def check_pointer_syntax(root: Path) -> tuple[list[str], list[tuple[str, int, str]]]:
    """返回 (错误, 引用列表[(file, line, path)])

    **判据精度说明（防误报）**：本项目的既有写法里，`{PRIVATE_ASSETS}` **单独出现**（不加斜杠）
    是**合法**的 —— 它指"环境变量本身／私有文件夹整体"（见 `TEMPLATES.md` §10.4 第 5 条）。
    例如「**`{PRIVATE_ASSETS}` 占位符字面量**」这种句子**不是**引用。
    ⇒ 判据只对**带路径的引用**（`{PRIVATE_ASSETS}/<路径>`）做**形式校验**；
      「只有斜杠没路径」也**只警告不判错**（属措辞不严谨，不是泄漏，也不是漏引）。
    """
    errs: list[str] = []
    warns: list[str] = []
    refs: list[tuple[str, int, str]] = []
    for p in iter_files(root):
        # ⚠️ **脚本自身必须排除**：本文件的注释/docstring 里**故意**写了各种**反例字面量**
        # （如 `{PRIVATE_ASSETS}\x.md`、`{PRIVATE_ASSETS}/../bad.md`）用于说明判据，
        # 若不排除，检查器会**把自己当成违规样本**（实测发生过：报了自己第 41 行）。
        if p.name == Path(__file__).name:
            continue
        try:
            text = p.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        for i, line in enumerate(text.splitlines(), 1):
            for m in PTR_RE.finditer(line):
                tail = m.group(1) or ""
                r = rel(root, p)
                if not tail:
                    # 合法/无害形态：只统计，不判错（见 docstring）
                    warns.append(f"{r}:{i} 未带路径（指代私有根整体，允许）")
                    continue
                sep, raw = tail[0], tail[1:]
                # **显式标注即可豁免**：文档里**讨论判据本身**时必然要写反例字面量
                # （如"初版只认 `/`，所以 `{PRIVATE_ASSETS}\x.md` 绕过了检查"）。
                # 这类行用「示例／反例／模式」标出即放行 —— 但**必须显式标**，
                # 且输出里会以 `[豁免]` 明示，**不会静默通过**（防"加个词就绕过"）。
                if any(k in line for k in PENDING_MARKS + ("反例",)):
                    warns.append(f"{r}:{i} 行内含豁免标记（示例/反例/模式/待建），跳过形式校验")
                    continue
                # —— 形式校验：**用原串**（含标点），否则反斜杠/`..` 会因"取干净段"而被掩盖 ——
                if sep == "\\":
                    errs.append(f"{r}:{i} 引用用了反斜杠（统一用正斜杠）")
                    continue
                if raw.startswith("/"):
                    errs.append(f"{r}:{i} 引用出现多余的斜杠（应为单斜杠相对路径）")
                    continue
                if ".." in raw.split("/"):
                    errs.append(f"{r}:{i} 引用含 `..`（不得越出私有根）")
                    continue
                # —— 存在性校验：**只取"路径安全字符"的连续段** ——
                # 为什么必须清洗：`{PRIVATE_ASSETS}/ASSETS_INVENTORY.md`**；路径真实值按…`
                # 这种写法里，贪心的尾类会把**后面的正文/markdown 记号**一起吞进来，
                # 拿去查文件必然不存在 ⇒ **假阳性**（实测：首版 resolve 报出 30+ 条假失败）。
                clean = re.match(r"[A-Za-z0-9_./\-]*", raw).group(0).rstrip("/")
                # 全是点/无实义字符（如 `{PRIVATE_ASSETS}/...`）＝**占位符形态**，属允许写法。
                # ⚠️ 不区分就会把它误判成"以 `..` 开头的绝对路径"（实测发生过，第 3 次改判据）。
                if not clean or not re.search(r"[A-Za-z0-9_\u4e00-\u9fff]", clean):
                    warns.append(f"{r}:{i} 未带具体路径（模板占位符/指代整体，允许）")
                    continue
                # ⚠️ **判据位置修正（2026-09-18 夜间 P3；由本轮新补的 `--selftest` 第 ④ 侧当场抓出）**：
                #    原判据写在 `clean` 上，而 `clean` 的字符类 `[A-Za-z0-9_./\-]` 里**没有 `:`**
                #    ⇒ `re.match(r"^[A-Za-z]:", clean)` **恒为 False（死分支，永不触发）**。
                #    实测后果**不止"少一条判据"**：`{PRIVATE_ASSETS}/D:/abs.md` 既不判红，
                #    还被**静默降级**成引用 `/D`（`clean` 截断在 `:` 处）——`resolve` 模式下
                #    这会去查一个**错误的目标**（假红与假绿都可能）。**判据静默失效最危险**。
                #    ⇒ 盘符判据改到**原串 `raw`** 上（与 `..` 判据同层：涉及"越界/绝对"的判断，
                #      一律用**未被清洗的原串**，清洗只用于"存在性查找"）。
                if re.match(r"^[A-Za-z]:", raw) or clean.startswith(".."):
                    errs.append(f"{r}:{i} 引用写成了绝对路径（只允许相对路径）")
                    continue
                refs.append((r, i, "/" + clean))
    if warns:
        print(f"[info] 指代私有根整体（未带路径）{len(warns)} 处 —— 允许，不计入错误")
    return errs, refs


# ---------- ② 泄漏扫描（与基线比对） ----------

BASELINE_PATH = Path("coordination/security/leak_baseline.txt")


def leak_scan_scope(root: Path, p: Path) -> bool:
    """**泄漏扫描的范围＝文档与治理**（不是源码）。

    为什么排除源码：源码里出现 `192.168.1.0/24` 这类**网段常量**是网络探测功能的**正常内容**
    （本项目的多内核互联 / 局域网发现本来就要这些），把它判成"泄漏"是**假阳性**。
    ⚠️ **教训（R16 同族）**：门禁**假阳性**会让人不再相信门禁，从而关掉它 ——
    **判据宁可窄而准，不要宽而吵**。
    """
    r = rel(root, p)
    if r.startswith(("coordination/", "docs/", "deploy/")):
        return True
    # 仓库根下的散装 markdown（如 README）
    return "/" not in r and p.suffix.lower() in {".md", ".txt"}


def load_baseline(root: Path) -> dict[tuple[str, str], int]:
    f = root / BASELINE_PATH
    base: dict[tuple[str, str], int] = {}
    if not f.exists():
        return base
    for line in f.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        parts = [x.strip() for x in line.split("|")]
        if len(parts) != 3:
            continue
        file_, pat, cnt = parts
        try:
            base[(file_, pat)] = int(cnt)
        except ValueError:
            continue
    return base


def scan_leaks(root: Path) -> dict[tuple[str, str], list[int]]:
    pats = list(LEAK_PATTERNS)
    u = _username_pattern()
    if u:
        pats.append(u)
    hits: dict[tuple[str, str], list[int]] = {}
    for p in iter_files(root):
        # 脚本自身不扫（含正则字面量，会自命中）
        if p.name == Path(__file__).name:
            continue
        if not leak_scan_scope(root, p):
            continue
        try:
            text = p.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        r = rel(root, p)
        for name, pat in pats:
            lines = [i for i, ln in enumerate(text.splitlines(), 1) if pat.search(ln)]
            if lines:
                hits[(r, name)] = lines
    return hits


# ---------- ③ 指针目标存在性（本机） ----------

def check_resolve(root: Path, private_root: Path, refs: list[tuple[str, int, str]]) -> list[str]:
    errs: list[str] = []
    # ④ 结构性隔离：私有根必须在仓库外（用 realpath 前缀比较；§10.4.4）
    try:
        rr = str(Path(os.path.realpath(root))).replace("\\", "/")
        pr = str(Path(os.path.realpath(private_root))).replace("\\", "/")
    except OSError as e:
        return [f"无法解析路径：{e}"]
    if pr.startswith(rr + "/"):
        errs.append("私有根落在**仓库之内** —— 违反结构性隔离（机制 17 §10.2④）")
    for file_, line, tail in refs:
        target = private_root / tail.lstrip("/")
        if target.exists():
            continue
        # —— **密文等价**（2026-09-17 新增；D39 执行后当场暴露）——
        # 背景：机制 20 的**逻辑名**是 `X.md`，但按 D39 执行 `purge` 后**磁盘上只剩 `X.md.enc`**。
        # 于是**执行 purge 之前写的文档**（含历史报告，按 D30-b 不追改）会指向一个"逻辑名存在、
        # 物理文件已换形态"的目标 ⇒ 若判红，就变成"**判据与机制不同步**"（不是文档错）。
        # 判据：目标不存在，但**同名 `.enc` 存在** ⇒ 视为**已解析**（内容确实在场，只是加密形态）。
        enc_target = target if target.suffix == ".enc" else target.with_name(target.name + ".enc")
        if enc_target.exists():
            print(f"  [密文等价] {file_}:{line} → 明文已按 D39 删除，命中间名密文 {enc_target.name}")
            continue
        # 「（待建）」标注允许缺失，但**必须显式标注**
        try:
            src = (root / file_).read_text(encoding="utf-8", errors="replace").splitlines()
            ctx = src[line - 1] if 0 < line <= len(src) else ""
        except OSError:
            ctx = ""
        if any(k in ctx for k in PENDING_MARKS):
            print(f"  [豁免] {file_}:{line} → 目标未建（行内已显式标注，允许）")
            continue
        errs.append(f"{file_}:{line} 引用目标不存在且未标「（待建）/（示例）/（模式）」：{tail}")
    return errs


# ================== [5] 密钥级别判据（机制 20 / C12 密钥专项级别）==================
#
# **为什么需要**：C12 里"密钥/账号/路径/网络细节 = ⛔ 机密"是**整类**规定，
# 但**密钥有三件东西、级别不一样**（本体 ⛔／指纹 🔒／路径 ⛔）——只靠报告的保密标记行
# **笼统带过**，读的人**无法判断**"这里出现的密钥指纹能不能拷"。⇒ 必须**行级标注**。
#
# **规则**：活文件中**凡出现「密钥」二字，该行必须带级别标识**（⛔／🔐／🔒／🔓）。
#
# **范围**：只扫**活文件**（治理层 ＋ `llm/` ＋ `workflows/` ＋ `TERMS.md` ＋ 顶层 `README`）。
# `reports/`／`instructions/`／`discussions/` 是**历史留痕、不追改**（沿用 D30-b）⇒ **不在范围内**。
KEY_LEVEL_MARKS = "⛔🔐🔒🔓"
KEY_LEVEL_SCOPE_TOP = (
    "coordination/CHARTER.md", "coordination/BASELINE.md", "coordination/CONSTRAINTS.md",
    "coordination/TEMPLATES.md", "coordination/ROADMAP.md", "coordination/TERMS.md",
    "coordination/advisor_brief.md", "coordination/ASSETS_INVENTORY.md", "README.md",
)


def check_key_levels(root: Path) -> list[str]:
    """返回"含密钥却无级别标识"的行（每项一段消息）。"""
    targets = [root / p for p in KEY_LEVEL_SCOPE_TOP]
    for sub in ("coordination/llm", "coordination/workflows"):
        d = root / sub
        if d.is_dir():
            targets += sorted(d.glob("*.md"))
    bad: list[str] = []
    scanned = 0
    for p in targets:
        if not p.exists():
            continue
        scanned += 1
        try:
            text = p.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        for i, line in enumerate(text.splitlines(), 1):
            if "密钥" in line and not any(m in line for m in KEY_LEVEL_MARKS):
                bad.append(
                    f"{rel(root, p)}:{i} 出现「密钥」但**无级别标识**（须标 ⛔/🔐/🔒/🔓；"
                    f"本体与路径=⛔、指纹=🔒）｜行：{line.strip()[:70]}"
                )
    print(f"[info] [5] 密钥级别判据：扫描活文件 {scanned} 个")
    return bad


# ================== [6] 指纹字面量判据（机制 20 / D39 归档 / T-023）==================
#
# **规则**：**密钥指纹（🔒）的字面量不得写入仓库** —— 只允许出现在
# 「本机私记」与「工具输出」中；文档里一律写 `〈本机私记〉` 占位。
#
# **为什么**：指纹虽然是单向哈希（不能反推密钥），但它一旦进仓库就**永久留在 git 历史里**，
# 而它的唯一用途又是"**核对备份**"——把它公开等于把"核对依据"也公开。
# 本轮（2026-09-17）用户明确收紧：**可拷，但不入仓库**。
#
# **怎么判**：一行里同时出现「指纹」与**16 位十六进制字面量** ⇒ 判红。
# （不做"按值匹配"——值只在本机私记里，脚本不该知道它；**判据不得依赖被保护对象本身**。）
FP_LINE = re.compile(r"\b[0-9a-fA-F]{16}\b")
FP_WORDS = ("指纹", "fingerprint")


def check_fingerprint_literal(root: Path) -> list[str]:
    """返回"含指纹字面量"的行（每项一段消息）。"""
    bad: list[str] = []
    scanned = 0
    for p in iter_files(root):
        if p.name == Path(__file__).name:      # 脚本自身带判据字样，跳过（避免自命中）
            continue
        try:
            text = p.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        scanned += 1
        for i, line in enumerate(text.splitlines(), 1):
            if any(w in line for w in FP_WORDS) and FP_LINE.search(line):
                bad.append(
                    f"{rel(root, p)}:{i} 出现**密钥指纹字面量**（须改为 `〈本机私记〉` 占位；"
                    f"T-023）｜行：{line.strip()[:70]}"
                )
    print(f"[info] [6] 指纹字面量判据：扫描文件 {scanned} 个")
    return bad


# ================== 判据自检（十一侧：正反对照 ∪ 真实仓库锚点）==================
#
# **为什么必须补**：机制 18／R64 的教训是「**自检与实跑口径必须同源**」——
# 判据**自己没被正反对照验过**时，它在 CI 里的绿只说明"**跑完了**"，不说明"**判对了**"。
# 反例（本仓真实教训）：常量多一字、夹具与常量同错 ⇒ 自检全绿而实跑判红。
# ⇒ 自检**必须**含 ① 双侧正反样例 ② **一个回读真实仓库的锚点**。
#
# **本自检的十条夹具侧 ＋ 一条锚点侧**（编号即输出里的序号）：
#   ①合规 ②反斜杠 ③`..` 越界 ④绝对路径 ⑤整体指代/占位（**防误报**）
#   ⑥显式豁免标记 ⑦泄漏正包/掩码负包 ⑧[5] 密钥级别正反 ⑨[6] 指纹正反
#   ⑩`resolve` 三包（目标存在／目标缺失／**私有根落在仓库内**）
#   ⑪**真实仓库锚点**：本仓 `--mode=syntax` 实跑 ⇒ 格式错 0 且解析到引用 >0
#
# ⚠️ **样本纪律**：夹具在**临时目录**里建，**不落真实仓库**；
#    ⑪ 对真实仓库**只读**（只解析、不写）。
# ⚠️ **样本值不得像真值**：⑨ 要求的"16 位十六进制"由 `"0" * 16` **构造**而非写字面量 ——
#    既是防"样本被误当真实指纹"，也顺带证明本判据**不依赖被保护对象本身**（见 §[6] 设计说明）。


def selftest() -> int:
    """十一侧自检（正反对照 ∪ 真实仓库锚点）。返回 0=判据可信；1=判据失效。"""
    import shutil
    import tempfile

    print("=== 判据自检（十一侧）===")
    res: list[tuple[str, bool, str]] = []

    def rec(label: str, ok: bool, detail: str) -> None:
        res.append((label, ok, detail))
        print(f"  {label} ⇒ {detail} {'✅' if ok else '❌'}")

    with tempfile.TemporaryDirectory() as td:
        repo = Path(td)
        (repo / "coordination" / "security").mkdir(parents=True)

        def put(name: str, text: str) -> Path:
            p = repo / name
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_text(text, encoding="utf-8")
            return p

        # ① 合规样例 ⇒ 0 错，且**确实解析到 2 处引用**（防"空跑也报绿"）
        put("coordination/probe.md",
            "标题\n引用 A {PRIVATE_ASSETS}/0_design/x.md 与 B {PRIVATE_ASSETS}/y.md\n")
        e, r = check_pointer_syntax(repo)
        rec("① 合规引用", len(e) == 0 and len(r) == 2,
            f"错 {len(e)}（期望 0）｜解析引用 {len(r)}（期望 2）")

        # ② 反斜杠写法 ⇒ 判红（R16 同族：初版只认 `/`，反斜杠整类漏报过）
        put("coordination/probe.md", "{PRIVATE_ASSETS}\\x.md\n")
        e, _ = check_pointer_syntax(repo)
        rec("② 反斜杠引用", any("反斜杠" in x for x in e),
            f"错 {len(e)}（期望 ≥1 且点名「反斜杠」）")

        # ③ `..` 越界 ⇒ 判红
        put("coordination/probe.md", "{PRIVATE_ASSETS}/../bad.md\n")
        e, _ = check_pointer_syntax(repo)
        rec("③ .. 越出私有根", any(".." in x for x in e), f"错 {len(e)}（期望 ≥1）")

        # ④ 写成绝对路径 ⇒ 判红
        put("coordination/probe.md", "{PRIVATE_ASSETS}/D:/abs.md\n")
        e, _ = check_pointer_syntax(repo)
        rec("④ 写成绝对路径", any("绝对路径" in x for x in e), f"错 {len(e)}（期望 ≥1）")

        # ⑤ **防误报**（阴性对照）：指代私有根整体 / 模板占位 ⇒ 合法，且**不计入引用**
        put("coordination/probe.md",
            "环境变量 {PRIVATE_ASSETS} 本身合法；模板 {PRIVATE_ASSETS}/... 也合法\n")
        e, r = check_pointer_syntax(repo)
        rec("⑤ 整体指代/占位", len(e) == 0 and len(r) == 0,
            f"错 {len(e)}（期望 0）｜引用 {len(r)}（期望 0，只提示不判错）")

        # ⑥ 行内显式标注（示例/反例）⇒ 放行；**必须显式**才放行（防"加个词就绕过"）
        put("coordination/probe.md", "反例：{PRIVATE_ASSETS}\\x.md 这种写法曾绕过初版判据\n")
        e, _ = check_pointer_syntax(repo)
        rec("⑥ 反例行显式标注", len(e) == 0, f"错 {len(e)}（期望 0）")

        # ⑦ 泄漏判据正反对照：**真名盘符**必须命中；**掩码示例**必须不命中
        put("coordination/leak_yes.md", "路径 D:/Users/fakeuser01/secret.md\n")
        put("coordination/leak_no.md", "掩码示例 C:\\Users\\<用户名>\\x.md（占位，非真名）\n")
        hits = scan_leaks(repo)
        yes = ("coordination/leak_yes.md", "win_abs_path") in hits
        no = any(k[0] == "coordination/leak_no.md" for k in hits)
        rec("⑦ 泄漏真名正/掩码负", yes and not no,
            f"真名命中={yes}（期望 True）｜掩码命中={no}（期望 False）")

        # ⑧ [5] 密钥级别：**同一次调用里同时验正反两包**（阳性行带 ⛔、阴性行不带）
        put("coordination/CHARTER.md",
            "样本 ⛔ 密钥本体与路径＝机密\n样本 密钥（无级别标识，应判红）\n")
        kb = check_key_levels(repo)
        rec("⑧ 密钥级别正反对照", len(kb) == 1,
            f"判红 {len(kb)} 行（期望恰好 1：只该抓「无标识」那行）")

        # ⑨ [6] 指纹字面量：正（16 位十六进制 ＋「指纹」⇒ 判红）／反（占位符 ⇒ 通过）
        sample_hex = "0" * 16
        put("coordination/probe.md", f"指纹：{sample_hex}（样本值，非真实指纹）\n")
        fp = check_fingerprint_literal(repo)
        rec("⑨a 指纹字面量判红", len(fp) == 1, f"判红 {len(fp)} 行（期望 1）")
        put("coordination/probe.md", "指纹：〈本机私记〉（占位符合规）\n")
        fp = check_fingerprint_literal(repo)
        rec("⑨b 指纹占位符通过", len(fp) == 0, f"判红 {len(fp)} 行（期望 0）")

        # ⑩ `resolve` 侧（**只在本机跑的那一半**，CI 覆盖不到 ⇒ 更需要自检）三包
        priv_out = Path(td).parent / (Path(td).name + "_priv")
        shutil.rmtree(priv_out, ignore_errors=True)
        priv_out.mkdir(parents=True)
        (priv_out / "exists.md").write_text("x", encoding="utf-8")
        e_ok = check_resolve(repo, priv_out, [("coordination/probe.md", 1, "/exists.md")])
        e_miss = check_resolve(repo, priv_out, [("coordination/probe.md", 1, "/missing.md")])
        priv_in = repo / "private_inside"          # 私有根落在**仓库之内** ⇒ 必须判红
        priv_in.mkdir()
        e_iso = check_resolve(repo, priv_in, [])
        rec("⑩ resolve 正反三包",
            len(e_ok) == 0 and len(e_miss) == 1 and any("结构性隔离" in x for x in e_iso),
            f"目标存在={len(e_ok)}（期望 0）｜目标缺失={len(e_miss)}（期望 1）｜"
            f"根在仓库内={len(e_iso)}（期望 ≥1 且点名「结构性隔离」）")
        shutil.rmtree(priv_out, ignore_errors=True)

    # ⑪ **真实仓库锚点**（R64：自检必须回读真实仓库，否则"夹具与常量同错"照样全绿）
    try:
        real = repo_root()
        e_real, refs_real = check_pointer_syntax(real)
        rec("⑪ 真实仓库锚点", len(e_real) == 0 and len(refs_real) > 0,
            f"格式错 {len(e_real)}（期望 0）｜解析引用 {len(refs_real)}（期望 >0）")
    except Exception as ex:  # pragma: no cover - 仅在非仓库目录内运行
        rec("⑪ 真实仓库锚点", False, f"无法解析仓库根（{type(ex).__name__}）⇒ 锚点未验，不得报绿")

    good = all(ok for _, ok, _ in res)
    print(f"\n自检结论：{'✅ 十一侧均符合预期' if good else '❌ 判据失效'}（通过 "
          f"{sum(1 for _, ok, _ in res if ok)}/{len(res)} 侧）")
    return 0 if good else 1


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--mode", choices=["syntax", "resolve"], default="syntax")
    ap.add_argument("--private-root", default=os.environ.get("PRIVATE_ASSETS", ""))
    ap.add_argument("--selftest", action="store_true", help="判据自检（十一侧正反对照）")
    args = ap.parse_args()

    if args.selftest:
        return selftest()

    root = repo_root()
    print(f"[info] 仓库根已解析（长度 {len(str(root))}）｜模式 = {args.mode}")

    errs, refs = check_pointer_syntax(root)
    print(f"[info] 指针引用 {len(refs)} 处 ｜ 格式错误 {len(errs)} 处")

    passed = True

    if args.mode == "syntax":
        base = load_baseline(root)
        hits = scan_leaks(root)
        new_leaks, reduced = [], []
        for key, lines in sorted(hits.items()):
            allowed = base.get(key, 0)
            if len(lines) > allowed:
                new_leaks.append((key, len(lines), allowed, lines[:5]))
            elif len(lines) < allowed:
                reduced.append((key, len(lines), allowed))
        for key, n, allowed, sample in new_leaks:
            passed = False
            locs = ",".join(str(x) for x in sample)
            print(f"[FAIL] 新增 ⛔ 泄漏：{key[0]}｜{key[1]}｜实测 {n} > 基线 {allowed}｜行 {locs}{'…' if n > 5 else ''}")
        for key, n, allowed in reduced:
            print(f"[info] 可精简基线：{key[0]}｜{key[1]}｜实测 {n} < 基线 {allowed}（建议下调）")
        if not new_leaks:
            print(f"[PASS] 未新增 ⛔ 泄漏（基线项 {len(base)} 条，本次命中 {len(hits)} 条）")
            # ★ **如实标注判据覆盖盲区**（R30）：`username_literal` 依赖**本机 home 名**，
            #    在 CI（`runneradmin`）上**根本无法匹配** ⇒ 该项在 CI 侧"看起来已修好"，
            #    实际只是**不可评估**。**不得**因此下调基线（C15：只减不增）。
            uname_item = [k for k in base if k[1] == "username_literal"]
            if uname_item:
                print(f"[info] ⚠️ **R30 判据盲区**：`username_literal` 项（{len(uname_item)} 条）"
                      f"依赖**本机 home 名**或**掩码名**，**CI 侧不可评估** ⇒ "
                      f"「命中数 < 基线」若出现在这一项，**代表不可评估，不代表已修好**。"
                      f"（本机需单独跑 `--mode=syntax` 才能覆盖该项）")

        # —— [5] 密钥级别判据（同时也算"泄漏"侧的一条硬判据）——
        key_bad = check_key_levels(root)
        if key_bad:
            passed = False
            for e in key_bad:
                print(f"[FAIL] [5] {e}")
        else:
            print("[PASS] [5] 密钥级别判据：活文件中凡含「密钥」的行**均带级别标识**")

        # —— [6] 指纹字面量判据（T-023：指纹可拷，但**不入仓库**）——
        fp_bad = check_fingerprint_literal(root)
        if fp_bad:
            passed = False
            for e in fp_bad:
                print(f"[FAIL] [6] {e}")
        else:
            print("[PASS] [6] 指纹字面量判据：仓库内**未出现**密钥指纹字面量（占位符 `〈本机私记〉` 合规）")

    if args.mode == "resolve":
        if not args.private_root:
            print("[FAIL] 未提供私有根：请设环境变量 PRIVATE_ASSETS 或用 --private-root 指定")
            print("       ⇒ 按机制 17 §10.2③：**如实标注「实现层未就位」，不得猜测、不得编造**（C5）")
            return 1
        pr = Path(args.private_root)
        errs += check_resolve(root, pr, refs)

    for e in errs:
        passed = False
        print(f"[FAIL] {e}")

    print("[PASS] 检查器通过" if passed else "[FAIL] 检查器未通过")
    return 0 if passed else 1


if __name__ == "__main__":
    sys.exit(main())
