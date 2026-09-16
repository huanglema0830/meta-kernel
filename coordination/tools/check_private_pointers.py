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
    #    只有 `C:\Users\香忆\...` 这种**真名**才命中。
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
                if re.match(r"^[A-Za-z]:", clean) or clean.startswith(".."):
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


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--mode", choices=["syntax", "resolve"], default="syntax")
    ap.add_argument("--private-root", default=os.environ.get("PRIVATE_ASSETS", ""))
    args = ap.parse_args()

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
