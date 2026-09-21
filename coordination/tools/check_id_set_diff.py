#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""
编号集差集判据（★ 先只告警，不判红 · 2026-09-21 · 本轮 §三）

【目的】
  自动检查「**引用的编号**」与「**登记的编号**」是否一致 —— 找出「被引用但没有登记」的编号。

【口径 · 登记的四形态（★ 本判据的核心）】
  登记 = 下列四形态之并（**逐行、行首锚**）：
    A 表格首格行      `| **Rxx** | …`
    B 表格第 2 列起   `| 7 | Rxx …`        （第 1 列后紧跟编号）
    C 分条列表        `- **Rxx**：…` / `- **Rxx ★（…）**：…`
    D 节标题          `### 12.1 Rxx（本轮新增）`
  ★ 依据：`coordination/reports/2026-09-21_R编号缺口核对稿_二.md` §一 ——
    **只认 A（表格首格行）会造出大量假阴性**：历轮 R 条目**大量采用 C（分条列表）与 D（节标题）形态**
    （R64–R68 = C；R69/R72 = D）。这正是前稿把「有登记」误读为「缺口」的病根（R83 / R87 同族）。

【范围】
  全仓 `*.md`（排除 `.git` / `target` / `node_modules` / `__pycache__` / `.workbuddy`）。

【★ MECH（机制）族 —— 区段限定专用解析（2026-09-21 裁定「选项 B」）】
  MECH 的编号**字面是裸数字**（`CHARTER.md` 机制表首格 `| 1 |`…，无族名）⇒ 全仓裸露解析会噪声爆炸。
  因此：**登记侧**只在 `CHARTER.md`「## 三、全部机制」**区段内**解析表格首格裸数字；**引用侧**用「机制 N」字面（自带族名）。
  **空转防护**：区段解析到 **0 行** ⇒ 判「空转告警」（**与「通过」取不同值**，C-04 / R83）。

【输出与纪律】
  - 每族输出「引用但无登记」清单（**告警**）；退出码**恒 0**（**先只告警**）；`--strict` 为预留（判红）。
  - **触发再评估**：本判据的告警命中**连续 N 轮为 0** ⇒ 可考虑转判红（N=5；登记于 `BASELINE §四十四`）。
  - ★ **自指污染**（已观察，暂不专项处置）：判据会把「讨论其自身告警的文本」纳入输入 ⇒ 告警数受报告内容影响
    （登记于 `BASELINE §四十四 / §四十五`）。

【自检】
  `--selftest`：正反对照**八侧** ——
    ① 有引用、无登记 ⇒ **必须**出现在差集（正例）
    ②③④ 分别以 A / C / D 形态登记 ⇒ **不得**出现在差集（反例；★ ③④ 是本判据的关键）
    ⑤ MECH 引用无登记 ⇒ 必须告警；⑥ MECH 区段首格登记 ⇒ 不告警；
    ⑦ MECH **区段外**裸数字不得入登记（区段限定生效）；⑧ MECH 空区段 ⇒ 登记为空（触发空转告警）
"""
import io
import os
import re
import sys
import argparse

# ---- 族定义：编号正则 + 权威登记载体（相对仓库根，仅作"主要落点"提示）----
FAMILIES = {
    # ★ 族正则统一加 `(?![A-Za-z0-9])` 尾断言 —— 排除「模式写法」`T-00N`／`TERM-0NN`／`TERM-00N`
    #   （否则 `T-\d+` 会在 `T-00N` 上截出假编号 `T-00`）。
    "R":    {"pat": r"R\d+(?![A-Za-z0-9])",     "auth": ["coordination/BASELINE.md"]},
    "D":    {"pat": r"D\d+(?![A-Za-z0-9])",     "auth": ["coordination/BASELINE.md"]},
    "C":    {"pat": r"C\d+(?![A-Za-z0-9])",     "auth": ["coordination/CONSTRAINTS.md", "coordination/CHARTER.md"]},
    "T":    {"pat": r"T-\d+(?![A-Za-z0-9])",    "auth": ["coordination/TEMPLATES.md"]},
    "TERM": {"pat": r"TERM-\d+(?![A-Za-z0-9])", "auth": ["coordination/TERMS.md"]},
    # ★ MECH（机制编号）族 —— 首格为**纯数字**（无族名）⇒ **必须区段限定**（`special: "mech"`，见 `mech_seg_lines()`）。
    #   引用侧用「机制 N」字面（自带族名、全仓无歧义）；登记侧用 `CHARTER.md`「## 三、全部机制」区段内表格首格裸数字。
    #   ★ 2026-09-21 裁定（选项 B）：**区段限定专用解析**启用
    #     依据：`coordination/reports/2026-09-21_MECH族专用解析评估稿.md`（选项 B；备「解析到 0 行 ⇒ 告警」遵 C-04/R83）。
    "MECH": {"pat": r"机制\s*(\d+)(?![0-9])", "auth": ["coordination/CHARTER.md"], "special": "mech"},
}

SKIP_DIRS = {".git", "target", "node_modules", "__pycache__", ".workbuddy"}


def forms(pat):
    """四形态正则（行首锚）。返回 [(form, compiled)]"""
    idp = "(?P<id>%s)" % pat
    return [
        ("A", re.compile(r"^\|\s*[\*`]{0,4}" + idp + r"[\*`]{0,4}\s*\|")),  # 表格首格行（★ `**` 或反引号包裹均可省 —— 实测三写法并存）
        ("B", re.compile(r"^\|[^|]*\|\s*" + idp)),                     # 表格第 2 列起
        ("C", re.compile(r"^\s*[-*]\s*\*\*[^\n]*?" + idp)),            # 分条列表
        ("D", re.compile(r"^#{2,6}\s*.*?" + idp)),                     # 节标题
    ]


def scan(lines, pat, reg=True):
    """reg=True ⇒ 登记集（四形态）；reg=False ⇒ 引用集（任意出现）"""
    out = set()
    if reg:
        fs = forms(pat)
        for l in lines:
            for _, rx in fs:
                m = rx.match(l)
                if m:
                    out.add(m.group("id"))
    else:
        rx = re.compile(pat)
        for l in lines:
            for m in rx.finditer(l):
                out.add(m.group(0))
    return out


def read_lines(path):
    try:
        return io.open(path, encoding="utf-8", errors="replace").read().split("\n")
    except OSError:
        return []


def walk_md(repo):
    for root, dirs, files in os.walk(repo):
        dirs[:] = [d for d in dirs if d not in SKIP_DIRS]
        for fn in files:
            if fn.endswith(".md"):
                yield os.path.join(root, fn).replace("\\", "/")


def repo_root():
    here = os.path.dirname(os.path.abspath(__file__))        # coordination/tools
    return os.path.abspath(os.path.join(here, "..", ".."))


# ---------------- MECH 族：区段限定专用解析（★ 2026-09-21 裁定「选项 B」）----------------
# 为什么需要：MECH 的编号**字面是裸数字**（`| 1 |`…，无族名）⇒ 若在全仓按裸数字解析，任何数字
#   都会被当候选（噪声爆炸）；因此**必须区段限定**：只在 `CHARTER.md` 的「## 三、全部机制」区段内解析。
MECH_SEG_KEY = "全部机制"

def mech_seg_slice(lines):
    """从行列表切出「## 三、全部机制」区段（至下一个 `## ` 一级标题或 EOF）。找不到 ⇒ 返回 []。"""
    start = None
    for i, l in enumerate(lines):
        if re.match(r"^##\s", l) and MECH_SEG_KEY in l:
            start = i
            continue
        if start is not None and re.match(r"^##\s", l):
            return lines[start:i]
    return lines[start:] if start is not None else []

def mech_seg_lines(charter_path):
    return mech_seg_slice(read_lines(charter_path))

def scan_mech_seg(seg_lines):
    """区段内「表格首格纯数字」⇒ 登记集（`| 5 | …`）。★ 仅区段内 ⇒ 区段外裸数字不入集。"""
    out = set()
    rx = re.compile(r"^\|\s*[\*`]{0,4}(\d+)[\*`]{0,4}\s*\|")
    for l in seg_lines:
        m = rx.match(l)
        if m:
            out.add(int(m.group(1)))
    return out

def scan_mech_ref(all_lines):
    """全仓「机制 N」字面 ⇒ 引用集（自带族名，无歧义；数字后不能紧跟数字，排除 `机制 10` 误截 `机制 1`）。"""
    out = set()
    rx = re.compile(r"机制\s*(\d+)(?![0-9])")
    for l in all_lines:
        for m in rx.finditer(l):
            out.add(int(m.group(1)))
    return out



def selftest():
    fails = []
    pat = FAMILIES["R"]["pat"]

    # 侧① 正例：有引用、无登记 ⇒ 必须进差集
    lines = ["- 这里提到 R99（引用）", "正文 R99 又提一次"]
    reg = scan(lines, pat, reg=True)
    ref = scan(lines, pat, reg=False)
    ok = "R99" in (ref - reg)
    print("  侧①（正例·有引用无登记 ⇒ 必须告警）: %s  差集=%s" % ("PASS" if ok else "FAIL", sorted(ref - reg)))
    if not ok:
        fails.append("侧①")

    # 侧② 反例(a)：A 表格首格行 ⇒ 不告警
    lines = ["| **R99** | 说明 |", "见 R99"]
    reg = scan(lines, pat, reg=True)
    ref = scan(lines, pat, reg=False)
    ok = "R99" not in (ref - reg)
    print("  侧②（反例·A 表格首格行登记 ⇒ 不告警）: %s" % ("PASS" if ok else "FAIL"))
    if not ok:
        fails.append("侧②")

    # 侧③ 反例(b)：C 分条列表 ⇒ 不告警（★ 关键；R64–R68 的形态）
    lines = ["- **R99 ★（本轮最重要）**：某缺陷"]
    reg = scan(lines, pat, reg=True)
    ref = scan(lines, pat, reg=False)
    ok = "R99" not in (ref - reg)
    print("  侧③（反例·C 分条列表登记 ⇒ 不告警）: %s" % ("PASS" if ok else "FAIL"))
    if not ok:
        fails.append("侧③")

    # 侧④ 反例(c)：D 节标题 ⇒ 不告警（★ 关键；R69/R72 的形态）
    lines = ["### 12.1 R99（本轮新增）"]
    reg = scan(lines, pat, reg=True)
    ref = scan(lines, pat, reg=False)
    ok = "R99" not in (ref - reg)
    print("  侧④（反例·D 节标题登记 ⇒ 不告警）: %s" % ("PASS" if ok else "FAIL"))
    if not ok:
        fails.append("侧④")

    # ==== MECH 族（区段限定专用解析 · 选项 B）====
    # 侧⑤ 正例(MECH)：引用「机制 99」但区段内无 `| 99 |` ⇒ 必须进差集
    seg = ["| 5 | 机制五 |", "| 6 | 机制六 |"]
    reg = scan_mech_seg(seg)
    ref = scan_mech_ref(["- 见机制 99（引用）", "机制 5 已登记"])
    ok = 99 in (ref - reg) and 5 not in (ref - reg)
    print("  侧⑤（正例·MECH 引用无登记 ⇒ 必须告警）: %s  差集=%s" % ("PASS" if ok else "FAIL", sorted(ref - reg)))
    if not ok:
        fails.append("侧⑤")

    # 侧⑥ 反例(MECH)：引用「机制 5」且区段有 `| 5 |` ⇒ 不告警
    seg = ["| 5 | 机制五 |"]
    reg = scan_mech_seg(seg)
    ref = scan_mech_ref(["见机制 5"])
    ok = not (ref - reg)
    print("  侧⑥（反例·MECH 区段首格登记 ⇒ 不告警）: %s" % ("PASS" if ok else "FAIL"))
    if not ok:
        fails.append("侧⑥")

    # 侧⑦ 反例(区段限定)：区段**外**的裸数字 `| 99 |` 不得被登记（裸数字必须靠区段定身份）
    lines7 = ["## 一、别的", "| 99 | 区段外裸数字 |", "## 三、全部机制", "| 5 | 机制五 |", "## 四、结尾"]
    reg = scan_mech_seg(mech_seg_slice(lines7))
    ok = (reg == {5})
    print("  侧⑦（反例·区段限定：区段外裸数字不入登记）: %s  登记=%s" % ("PASS" if ok else "FAIL", sorted(reg)))
    if not ok:
        fails.append("侧⑦")

    # 侧⑧ 空转防护：区段解析到 0 行 ⇒ 登记应为空集（主循环据此判「空转告警」，≠ PASS）
    reg = scan_mech_seg(mech_seg_slice(["## 一、x", "无表"]))
    ok = (len(reg) == 0)
    print("  侧⑧（反例·MECH 空区段 ⇒ 登记为空 ⇒ 触发空转告警）: %s" % ("PASS" if ok else "FAIL"))
    if not ok:
        fails.append("侧⑧")

    print("=" * 60)
    if fails:
        print("自检结论：FAIL %d 项 %s" % (len(fails), fails))
        return 1
    print("自检结论：PASS（八侧：R 五族 1 正 + 3 反 ＋ MECH 3 反 + 1 空转防护）")
    return 0


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--selftest", action="store_true", help="自检（四侧正反对照）")
    ap.add_argument("--strict", action="store_true", help="（预留）有告警即判红；默认只告警")
    ap.add_argument("--limit", type=int, default=12, help="每族告警展示条数")
    a = ap.parse_args()
    if a.selftest:
        return selftest()

    repo = repo_root()
    print("=" * 66)
    print("编号集差集判据（★ 先只告警，不判红）   仓库根：%s" % os.path.basename(repo))
    print("=" * 66)

    files = list(walk_md(repo))
    all_lines = []
    for p in files:
        all_lines.extend(read_lines(p))
    print("扫描 %d 个 *.md ｜ 行数 %d" % (len(files), len(all_lines)))
    print("登记口径：A 表格首格行 ∪ B 表格第2列 ∪ C 分条列表 ∪ D 节标题（行首锚）")

    total = 0
    for fam, cfg in FAMILIES.items():
        if cfg.get("special") == "mech":
            # ★ MECH 族：**区段限定专用解析**（选项 B · 2026-09-21 裁定）
            seg = mech_seg_lines(os.path.join(repo, "coordination/CHARTER.md"))
            reg = scan_mech_seg(seg)
            ref = scan_mech_ref(all_lines)
            diff = sorted(ref - reg)
            print("\n【%s】引用 %d ｜ 登记 %d ｜ **引用但无登记 %d**" % (fam, len(ref), len(reg), len(diff)))
            print("    登记口径：`CHARTER.md`「## 三、全部机制」**区段内**表格首格裸数字（区段限定；区段行数 %d）" % len(seg))
            print("    主要落点：%s" % "、".join(cfg["auth"]))
            if not reg:
                # ★ 空转防护：解析到 0 行 ⇒ 与「通过」取不同值（C-04 / R83）
                print("    ⚠️ 区段解析到 **0 行** —— **判据空转**（不是 PASS；C-04/R83）")
                total += 1
            elif diff:
                total += len(diff)
                show = diff[:a.limit]
                print("    ⚠️ 告警：%s%s" % (", ".join(str(x) for x in show), " …" if len(diff) > a.limit else ""))
            else:
                print("    ✅ 无告警")
            continue
        ref = scan(all_lines, cfg["pat"], reg=False)
        reg = scan(all_lines, cfg["pat"], reg=True)
        diff = sorted(ref - reg, key=lambda s: (len(s), s))
        total += len(diff)
        print("\n【%s】引用 %d ｜ 登记 %d ｜ **引用但无登记 %d**" % (fam, len(ref), len(reg), len(diff)))
        print("    主要落点：%s" % "、".join(cfg["auth"]))
        if diff:
            show = diff[:a.limit]
            print("    ⚠️ 告警：%s%s" % (", ".join(show), " …" if len(diff) > a.limit else ""))
        else:
            print("    ✅ 无告警")

    print("\n" + "=" * 66)
    print("告警合计：%d" % total)
    print("★ 本判据**先只告警、不判红**（退出码恒 0）；再评估＝告警命中连续 N 轮为 0（N 待定）。")
    print("=" * 66)
    if a.strict and total:
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
