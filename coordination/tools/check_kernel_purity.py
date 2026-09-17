#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""检查器 · **执行体归属判据**（`TEMPLATES.md` §十三／机制 21）

## 它判什么

**内核只算，不做感知与动作。** 内核三个 crate（`meta-kernel-core`／`-nostd`／`-mem`）的
**非测试代码区**里，**不得出现**下列七类「感知／动作」用法：

| 类 | 模式（示例） | 为什么不能在内核 |
|---|---|---|
| 文件 IO | `fs::` / `File::` / `read_to_string` / `write_all` | 内核不持文件系统 |
| 网络 | `TcpStream` / `UdpSocket` / `SocketAddr` / `reqwest` | 内核不持网络栈 |
| 进程 | `Command::new` / `std::process` / `Child` | 内核不拉起进程 |
| 时钟 | `SystemTime` / `Instant::now` / `std::time` | 内核不持墙钟 |
| 环境变量 | `env::var` / `std::env`（**`env!` 编译期宏例外**） | 内核无运行环境 |
| 线程 | `std::thread` / `thread::spawn` | 内核无 std 线程 |
| 系统调用 | `libc::` / `syscall` | 属宿主/驱动层 |

**判据形态**：命中数必须**等于白名单条目**（`coordination/security/kernel_purity_baseline.txt`）。
**多一条 ⇒ 判红**（新增了感知/动作）；**少一条 ⇒ 只提示**（可精简白名单，**只减不增**）。

## 为什么这条判据必要

1. **实测**：内核侧这三类用法**曾是 0**——它决定"哪些活只能 WorkBuddy 干、哪些可以下沉内核"。
   没有机器判据时，这条边界只写在文档里，**下一个人加一行 `std::fs` 也不会有人发现**。
2. **防混名**：「通用工作流」同时是 ①**方法论骨架**（公开层，给人/Agent 用）与 ②**要实现的功能**
   （实现层，当前占位）。若无判据，很容易被读成"内核已具备工作流能力"（**R36**）。
3. **同向约束**：C1（零依赖）与裸机目标本已隐含它，但**编译不过 ≠ 设计错误**——
   本事要做的是**在写之前就问归属**，而不是等编译器报错。

## 用法

    python coordination/tools/check_kernel_purity.py            # 判据（入 CI）
    python coordination/tools/check_kernel_purity.py --selftest # 阴阳自检（注入正反两侧样例）
    python coordination/tools/check_kernel_purity.py --report   # 只报告（不判红，供调研用）

## 口径（**必须写明**，R35 教训）

* `处` ＝ 命中**行数**（同一行多次出现只算 1 处）。
* 统计范围 ＝ **非注释行**（整行 `//` 或 `*` 开头）且**非 `#[cfg(test)]` 块**。
* 测试区命中**单独列出**、**不判红**（host 侧测试读文件是合法的；但**必须可见**，不得隐藏）。
"""

from __future__ import annotations

import io
import os
import re
import sys

# —— 内核 crate（**这三个是"只算"的执行体**；宿主/工作台不在此范围）——
KERNEL_CRATES = [
    "meta-kernel-core/src",
    "meta-kernel-core-nostd/src",
    "meta-kernel-mem/src",
]

# —— 七类「感知／动作」模式 ——
# ⚠️ 每条都用**词边界**（C9 教训：裸 `grep unsafe` 会误报变量名）
PATTERNS: list[tuple[str, str, str]] = [
    ("file_io",  r"\bfs::|\bFile::|read_to_string|\bwrite_all\b|std::fs", "文件 IO"),
    ("net",      r"\bTcpStream\b|\bUdpSocket\b|\bSocketAddr\b|\bstd::net\b|\breqwest\b|\bhyper\b", "网络"),
    ("process",  r"Command::new|std::process|\bChild\b", "进程"),
    ("clock",    r"SystemTime|Instant::now|std::time", "时钟"),
    ("env",      r"env::var|std::env", "环境变量（`env!` 编译期宏**不在**此列）"),
    ("thread",   r"std::thread|thread::spawn", "线程"),
    ("syscall",  r"\blibc::|\bsyscall\b", "系统调用"),
]

BASELINE_REL = "coordination/security/kernel_purity_baseline.txt"


def repo_root() -> str:
    import subprocess

    out = subprocess.run(
        ["git", "rev-parse", "--show-toplevel"], capture_output=True, text=True
    ).stdout.strip()
    return out or os.getcwd()


def strip_test_blocks(txt: str) -> str:
    """把 `#[cfg(test)]` 起的块整体挖空（花括号配对），保留行数（用空行占位）。"""
    spans: list[tuple[int, int]] = []
    for m in re.finditer(r"#\[cfg\(test\)\]", txt):
        j = txt.find("{", m.end())
        if j < 0:
            continue
        depth = 0
        k = j
        while k < len(txt):
            if txt[k] == "{":
                depth += 1
            elif txt[k] == "}":
                depth -= 1
                if depth == 0:
                    break
            k += 1
        spans.append((m.start(), min(k + 1, len(txt))))
    if not spans:
        return txt
    out = []
    last = 0
    for a, b in sorted(spans):
        if a < last:
            continue
        out.append(txt[last:a])
        out.append("\n" * txt[a:b].count("\n"))
        last = b
    out.append(txt[last:])
    return "".join(out)


def code_lines(txt: str) -> list[tuple[int, str]]:
    """返回 (行号, 行内容) —— 已剔除整行注释。"""
    res = []
    for i, l in enumerate(txt.split("\n"), 1):
        s = l.strip()
        if not s or s.startswith("//") or s.startswith("*") or s.startswith("/*"):
            continue
        res.append((i, l))
    return res


def scan(root: str) -> tuple[dict[tuple[str, str, str], list[int]], dict[tuple[str, str], list[int]]]:
    """返回 (非测试命中, 测试区命中)。键＝(crate相对根, 相对文件, 类)。"""
    primary: dict[tuple[str, str, str], list[int]] = {}
    tests: dict[tuple[str, str], list[int]] = {}
    for crate in KERNEL_CRATES:
        d = os.path.join(root, crate)
        if not os.path.isdir(d):
            continue
        for dp, _, fs in os.walk(d):
            for f in sorted(fs):
                if not f.endswith(".rs"):
                    continue
                fp = os.path.join(dp, f)
                rel = os.path.relpath(fp, d).replace("\\", "/")
                txt = io.open(fp, encoding="utf-8", errors="replace").read()
                body = strip_test_blocks(txt)
                for name, pat, _desc in PATTERNS:
                    hit = [n for n, l in code_lines(body) if re.search(pat, l)]
                    if hit:
                        primary[(crate, rel, name)] = hit
                # 测试区：正文挖空后剩下的（挖空部分的行号无法直接还原，用整文减正文的近似）
                for name, pat, _desc in PATTERNS:
                    hit = [n for n, l in code_lines(txt) if re.search(pat, l)]
                    in_body = set(primary.get((crate, rel, name), []))
                    rest = [n for n in hit if n not in in_body]
                    if rest:
                        tests[(crate, rel, name)] = rest
    return primary, tests


def load_baseline(root: str) -> dict[str, int]:
    """白名单：`<crate>/<file>|<类>|<条数>` 外加 `# 理由`。"""
    p = os.path.join(root, BASELINE_REL)
    if not os.path.exists(p):
        return {}
    out: dict[str, int] = {}
    for l in io.open(p, encoding="utf-8").read().split("\n"):
        s = l.strip()
        if not s or s.startswith("#"):
            continue
        parts = [x.strip() for x in s.split("|")]
        if len(parts) < 3:
            continue
        out[f"{parts[0]}|{parts[1]}"] = int(parts[2])
    return out


def check(root: str, verbose: bool = True) -> int:
    primary, tests = scan(root)
    base = load_baseline(root)
    print(f"[info] 内核 crate 数 = {len(KERNEL_CRATES)}｜模式类 = {len(PATTERNS)}｜{BASELINE_REL} 条目 = {len(base)}")

    # 归并成 <crate>/<file>|<类> → 行号列表
    agg: dict[str, list[int]] = {}
    for (crate, rel, name), lines in primary.items():
        agg.setdefault(f"{crate}/{rel}|{name}", []).extend(lines)

    new_hits, reduced = [], []
    for key, lines in sorted(agg.items()):
        allowed = base.get(key, 0)
        if len(lines) > allowed:
            new_hits.append((key, len(lines), allowed, sorted(lines)[:6]))
        elif len(lines) < allowed:
            reduced.append((key, len(lines), allowed))

    for key, n, allowed, sample in new_hits:
        print(f"[FAIL] 内核出现「感知/动作」用法：{key}｜实测 {n} > 白名单 {allowed}｜行 {sample}")
    for key, n, allowed in reduced:
        print(f"[info] 可精简白名单：{key}｜实测 {n} < 白名单 {allowed}（建议下调，**只减不增**）")

    if tests:
        print(f"[info] 测试区命中（**不判红**，仅列出以便可见）：{len(tests)} 处")
        for (crate, rel, name), lines in sorted(tests.items())[:10]:
            print(f"        {crate}/{rel}｜{name}｜行 {sorted(lines)[:4]}")

    if verbose and not new_hits:
        print(f"[PASS] 内核纯度：{len(agg)} 条命中**全在白名单内**（新增感知/动作 = 0）")
    return 0 if not new_hits else 1


def selftest() -> int:
    """**阴阳自检**：注入正反两侧样例，验证判据既不空转、也不误报。

    ① 阴性对照（必须判红）：往内核 crate 里塞一行 `std::fs::read(...)`
    ② 阳性对照（必须不误报）：塞**注释行**含 `std::fs`、以及合法的 `alloc::`／`core::`／`env!`
    ③ 白名单不空转：确认"合法样本 + 白名单"下判据为 PASS
    """
    root = repo_root()
    target_crate = "meta-kernel-core/src"
    probe = os.path.join(root, target_crate, "__purity_selftest.rs")
    ok = True

    def run_once() -> int:
        return check(root, verbose=False)

    base_rc = run_once()
    print(f"  ① 基线态：exit={base_rc}（预期 0）")
    ok &= base_rc == 0

    try:
        # —— 阴性对照：真违规 ——
        io.open(probe, "w", encoding="utf-8", newline="\n").write(
            "pub fn evil() -> u8 {\n"
            "    let _s = std::fs::read_to_string(\"x\").unwrap_or_default();\n"
            "    1\n"
            "}\n"
        )
        rc_bad = run_once()
        print(f"  ② 注入真违规（std::fs::read_to_string）：exit={rc_bad}（预期 1）")
        ok &= rc_bad == 1

        # —— 阳性对照：注释里的同名串**不得**误报 ——
        io.open(probe, "w", encoding="utf-8", newline="\n").write(
            "// 说明：本模块**不使用** std::fs，也不做 std::env 读取（这是注释，不是用法）\n"
            "pub fn fine() -> u32 {\n"
            "    let v: alloc::vec::Vec<u8> = alloc::vec![1u8, 2];\n"
            "    let n: u32 = core::cmp::max(v.len() as u32, 1);\n"
            "    let _ver: &str = env!(\"CARGO_PKG_VERSION\");\n"
            "    n\n"
            "}\n"
        )
        rc_ok = run_once()
        print(f"  ③ 注入合法写法（注释含同名字样 + alloc/core/env!）：exit={rc_ok}（预期 0）")
        ok &= rc_ok == 0

        # —— 测试区不得判红（口径可见性）——
        io.open(probe, "w", encoding="utf-8", newline="\n").write(
            "pub fn f() -> u32 { 1 }\n\n"
            "#[cfg(test)]\n"
            "mod t {\n"
            "    #[test]\n"
            "    fn reads_file() {\n"
            "        let _ = std::fs::read_to_string(\"x\");\n"
            "    }\n"
            "}\n"
        )
        rc_test = run_once()
        print(f"  ④ 违规只在 #[cfg(test)] 内：exit={rc_test}（预期 0，但须在 [info] 可见）")
        ok &= rc_test == 0
    finally:
        if os.path.exists(probe):
            os.remove(probe)

    rc_clean = run_once()
    print(f"  ⑤ 清理后：exit={rc_clean}（预期 0）")
    ok &= rc_clean == 0

    print(f"\n  自检结论：{'✅ 四侧均符合预期（判据既不空转、也不误报）' if ok else '❌ 有偏离预期的一侧，判据不可用'}")
    return 0 if ok else 1


def main() -> int:
    args = sys.argv[1:]
    root = repo_root()
    if "--selftest" in args:
        return selftest()
    if "--report" in args:
        check(root)
        return 0
    return check(root)


if __name__ == "__main__":
    raise SystemExit(main())
