#!/usr/bin/env python3
"""机制 18 / C13「顾问阅读指令」形态判据（2026-09-19 建立 · 丁案）。

## 一句话判据
`coordination/reports/*.md` 里，**除历史豁免名单外**，每份的**字面第 0 行**必须**逐字等于**
`TEMPLATES.md §二 第〇项` 的固定文本，且该指令块**整块逐字一致**。

## 口径（三条，写清以免误用）
1. **期望值不手写**：从唯一权威 `coordination/TEMPLATES.md` 的 ```markdown 围栏块**抽取**
   （符合 **D40「清单不手写」** 与 **C19「判据↔判据同源」**）。
2. **范围**：`coordination/reports/*.md`（**报告**＝最主要的"输出"形态）。
   ⚠️ C13 原文的范围是"**全部输出**"；本判据**只覆盖报告**，其余目录（`discussions/`／
   `instructions/`／`coordination/*.md`）**尚未覆盖** —— 这是**已知的窄范围**（fail-closed 于本范围）。
3. **历史豁免（丁案 · 不回填）**：既有报告**不回填**（改历史快照＝篡改留痕，且对"顾问已读过的报告"
   零收益）；其**实名清单**在 `coordination/security/reading_directive_exempt.txt`，
   **只减不增**（新增条目＝判红）。

## 检查项
- **P1 位置**：非豁免文件的**字面第 0 行**必须起于固定文本（`startswith`）。
- **P2 逐字**：非豁免文件中的指令块**整块**必须等于模板块。
- **P3 豁免名单只减不增**：条目数 **≤ 冻结条目数**（文件头声明）；且**每条都必须仍存在**。

## 用法
```
check_reading_instruction.py              # 跑判据（实跑）
check_reading_instruction.py --selftest   # 判据自检（正反两侧）
check_reading_instruction.py --freeze     # 按当前实测重写豁免名单（＝冻结基线；须显式调用）
```

退出码：0 = PASS；1 = FAIL；2 = 环境/文件问题（也算失败）
"""

from __future__ import annotations

import argparse
import glob
import io
import os
import re
import sys

TEMPLATES_REL = "coordination/TEMPLATES.md"
REPORTS_GLOB = "coordination/reports/*.md"
EXEMPT_REL = "coordination/security/reading_directive_exempt.txt"

START = "【顾问阅读指令 · 请先执行】"
END = "如未执行以上步骤，视为阅读不到位。"
FROZEN_MARK = "# 冻结条目数:"


def repo_root() -> str:
    import subprocess

    out = subprocess.run(
        ["git", "rev-parse", "--show-toplevel"], capture_output=True, text=True
    ).stdout.strip()
    return out or os.getcwd()


def read_text(path: str) -> str:
    return io.open(path, encoding="utf-8").read()


def canon_from_templates(path: str) -> str:
    """从 TEMPLATES.md 抽取含起始标记的 markdown 围栏块（★ 唯一权威期望值）。"""
    s = read_text(path)
    for m in re.finditer(r"```markdown\r?\n(.*?)\r?\n```", s, re.S):
        if START in m.group(1):
            return m.group(1)
    raise SystemExit("[FATAL] TEMPLATES.md 中未找到含「顾问阅读指令」的 markdown 围栏块")


def block_of(text: str):
    """返回 (块文本 | None | 'TRUNCATED')。"""
    i = text.find(START)
    if i < 0:
        return None
    j = text.find(END, i)
    if j < 0:
        return "TRUNCATED"
    return text[i : j + len(END)]


def load_exempt(path: str):
    """返回 (名单 list[str], 冻结条目数 int|None)。"""
    if not os.path.exists(path):
        return [], None
    names, frozen = [], None
    for line in read_text(path).splitlines():
        t = line.strip()
        if t.startswith(FROZEN_MARK):
            try:
                frozen = int(t.split(":", 1)[1].strip())
            except ValueError:
                frozen = None
        elif t.startswith("#") or not t:
            continue
        else:
            names.append(t)
    return names, frozen


def write_exempt(path: str, names) -> None:
    hdr = [
        "# 顾问阅读指令 · 历史豁免名单（C13 丁案 · 2026-09-19）",
        "#",
        "# 一行一个**报告文件名**（相对 coordination/reports/）；只列**实测不合规**的既有报告。",
        "# 口径：**历史快照不回填**（改它＝篡改留痕，且对已读过的报告零收益）。",
        "#",
        "# ⚠️ **只减不增**：本名单的条目数**不得超过**下方冻结条目数；",
        "#    新增条目一律判红（＝把「新报告不合规」伪装成「历史豁免」）。",
        "#    要减少条目 ⇒ 把该报告修到合规后从本名单删除（只减）。",
        "# 生成方式：`check_reading_instruction.py --freeze`（**不手写**，D40）。",
        "",
        FROZEN_MARK + " %d" % len(names),
        "",
    ]
    io.open(path, "w", encoding="utf-8", newline="\n").write(
        "\n".join(hdr + sorted(names)) + "\n"
    )


def check(root: str):
    """核心判定。返回 (errs, infos)。**自检与实跑共用本函数**（C18）。"""
    errs, infos = [], []
    canon = canon_from_templates(os.path.join(root, TEMPLATES_REL))
    infos.append("模板块字符数 = %d（自 TEMPLATES.md 抽取）" % len(canon))

    exempt, frozen = load_exempt(os.path.join(root, EXEMPT_REL))
    exempt_set = set(exempt)
    if frozen is None:
        errs.append("[豁免名单] 缺 %s 行（无法判定「只减不增」）" % FROZEN_MARK)
    elif len(exempt) > frozen:
        errs.append("[豁免名单] 条目 %d > 冻结 %d ⇒ **只减不增被破**" % (len(exempt), frozen))

    reports = sorted(os.path.basename(p) for p in glob.glob(os.path.join(root, REPORTS_GLOB)))
    infos.append("报告数 = %d｜豁免条目 = %d（冻结 %s）" % (len(reports), len(exempt), frozen))

    if not reports:
        errs.append("[范围] %s 未匹配到任何文件（范围失效 ⇒ 判据空转）" % REPORTS_GLOB)

    checked = 0
    for name in reports:
        if name in exempt_set:
            continue
        checked += 1
        text = read_text(os.path.join(root, "coordination", "reports", name))
        if not text.startswith(START):
            errs.append("[P1 位置] %s 的字面第 0 行不是固定文本" % name)
            continue
        blk = block_of(text)
        if blk == "TRUNCATED":
            errs.append("[P2 逐字] %s 的指令块缺收尾句（截断）" % name)
        elif blk != canon:
            errs.append("[P2 逐字] %s 的指令块与模板不一致" % name)
    infos.append("受检（非豁免）报告数 = %d" % checked)

    for name in exempt:
        if not os.path.exists(os.path.join(root, "coordination", "reports", name)):
            errs.append("[P3 名单陈旧] 豁免名单中的 %s 已不存在 ⇒ 应删除该条目（只减）" % name)

    return errs, infos


def freeze(root: str) -> int:
    canon = canon_from_templates(os.path.join(root, TEMPLATES_REL))
    names = []
    for p in glob.glob(os.path.join(root, REPORTS_GLOB)):
        text = read_text(p)
        blk = block_of(text)
        if text.startswith(START) and blk == canon:
            continue  # 合规 ⇒ 不入名单
        names.append(os.path.basename(p))
    write_exempt(os.path.join(root, EXEMPT_REL), names)
    print("[OK] 已写 %s｜条目 = %d" % (EXEMPT_REL, len(names)))
    return 0


def do_check(root: str) -> int:
    errs, infos = check(root)
    for s in infos:
        print("[info] " + s)
    if errs:
        for e in errs:
            print("[FAIL] " + e)
        print("结果：FAIL（%d 项）" % len(errs))
        return 1
    print("结果：PASS")
    return 0


def selftest() -> int:
    """自检：**在临时仓库夹具上跑与实跑同一函数 check()**（C18 同源）。

    阳性：合规报告 ⇒ 无 P1/P2 错误
    阴性①：第 0 行被前置标题 ⇒ P1 报红
    阴性②：块被截断 ⇒ P2 报红
    阴性③：豁免名单条目数超冻结 ⇒ P3 报红
    阴性④：豁免名单列了不存在的文件 ⇒ 陈旧条目报红
    """
    import shutil
    import tempfile

    src_root = repo_root()
    canon = canon_from_templates(os.path.join(src_root, TEMPLATES_REL))
    ok = bad = 0

    def case(name, expect_err, setup):
        nonlocal ok, bad
        d = tempfile.mkdtemp()
        try:
            os.makedirs(os.path.join(d, "coordination", "reports"), exist_ok=True)
            os.makedirs(os.path.join(d, "coordination", "security"), exist_ok=True)
            io.open(os.path.join(d, TEMPLATES_REL), "w", encoding="utf-8", newline="\n").write(
                "# T\n\n```markdown\n" + canon + "\n```\n"
            )
            setup(d)
            errs, _ = check(d)
            hit = any(expect_err in e for e in errs) if expect_err else (not errs)
            print("[SELFTEST] %s: 期望%s 实测 errs=%d %s" % (
                name, ("命中 " + expect_err) if expect_err else "无错",
                len(errs), "OK" if hit else "BAD"))
            if hit:
                ok += 1
            else:
                bad += 1
                for e in errs:
                    print("      -", e)
        finally:
            shutil.rmtree(d, ignore_errors=True)

    def w_report(d, name, content):
        io.open(os.path.join(d, "coordination", "reports", name), "w",
                encoding="utf-8", newline="\n").write(content)

    def w_exempt(d, names, frozen):
        lines = ["# t", FROZEN_MARK + " %d" % frozen, ""] + list(names)
        io.open(os.path.join(d, EXEMPT_REL), "w", encoding="utf-8", newline="\n").write(
            "\n".join(lines) + "\n")

    case("阳性 合规报告", None,
         lambda d: (w_report(d, "a.md", canon + "\n\n正文\n"), w_exempt(d, [], 0)))
    case("阴性① 第0行是标题", "[P1 位置]",
         lambda d: (w_report(d, "a.md", "# 标题\n\n" + canon + "\n"), w_exempt(d, [], 0)))
    case("阴性② 块被截断", "[P2 逐字]",
         lambda d: (w_report(d, "a.md", START + "\n\n1. xx\n"), w_exempt(d, [], 0)))
    case("阴性③ 豁免超冻结", "只减不增被破",
         lambda d: (w_report(d, "a.md", "x\n"), w_exempt(d, ["a.md", "b.md"], 1)))
    case("阴性④ 豁免名单陈旧", "[P3 名单陈旧]",
         lambda d: (w_report(d, "a.md", canon + "\n"), w_exempt(d, ["gone.md"], 5)))

    print("[SELFTEST] 合计 通过=%d 失败=%d（应为 通过=5 失败=0）" % (ok, bad))
    return 0 if bad == 0 else 1


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--selftest", action="store_true")
    ap.add_argument("--freeze", action="store_true", help="按当前实测重写豁免名单（冻结基线）")
    a = ap.parse_args()
    if a.selftest:
        return selftest()
    root = repo_root()
    if a.freeze:
        return freeze(root)
    return do_check(root)


if __name__ == "__main__":
    sys.exit(main())
