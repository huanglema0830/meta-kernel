#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""
编号集差集判据（★ 先只告警，不判红 · 2026-09-21 · 本轮 §三）

【目的】
  自动检查「**引用的编号**」与「**登记的编号**」是否一致 —— 找出「被引用但没有登记」的编号。

【口径 · 登记的四形态（★ 本判据的核心）】
  登记 = 下列四形态之并（**逐行、行首锚**；★ C/D 已于 2026-09-21 **收紧**，见文末 v0.309 修订）：
    A 表格首格行      `| **Rxx** | …`
    B 表格第 2 列起   `| 7 | Rxx …`        （第 1 列后紧跟编号）
    C 分条列表        `- **Rxx**：…`（★ **编号须紧邻行首分条标记**；旧的"任意位置"写法已收紧）
    D 节标题          `### 12.1 Rxx（本轮新增）`（★ **编号须在标题前段**，允许段号／`★` 前缀）
  ★ 依据：`coordination/reports/2026-09-21_R编号缺口核对稿_二.md` §一 ——
    **只认 A（表格首格行）会造出大量假阴性**：历轮 R 条目**大量采用 C（分条列表）与 D（节标题）形态**
    （R64–R68 = C；R69/R72 = D）。这正是前稿把「有登记」误读为「缺口」的病根（R83 / R87 同族）。
  ★ **收紧的实测代价（可复现）**：收紧后 R **90/90**、D **55/55**、TERM **14/14** 三族**不受影响**；
    C 登记 23 → 22（新增 1 条告警 `C22`）、T 登记 44 → 43（**假登记消失**，新增 1 条告警 `T-00`）
    ⇒ **收紧不引发大面积回归，且暴露了 2 条被"宽登记"掩盖的元引用告警**。

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

【★ T-041「告警扫描范围限定」（2026-09-21 落地）】
  扫描前，先剔除 5 层文本（元标注块／待裁定段／问答段／举例引用段／讨论段）—— 清单在
  `coordination/tools/scan_exclude.json`（**机器可读**）；**每次扫描打印「已剔除段落」**；规则本身见 `TEMPLATES.md §十八`。
  ★ **只剔引用集，不剔登记集**。三条约束齐备 ⇒ 剔除**不是隐藏规则**（R73 教训）。

【自检】
  `--selftest`：正反对照**十九侧** ——
    ① 有引用、无登记 ⇒ **必须**出现在差集（正例）
    ②③④ 分别以 A / C / D 形态登记 ⇒ **不得**出现在差集（反例；★ ③④ 是本判据的关键）
    ⑤ MECH 引用无登记 ⇒ 必须告警；⑥ MECH 区段首格登记 ⇒ 不告警；
    ⑦ MECH **区段外**裸数字不得入登记（区段限定生效）；⑧ MECH 空区段 ⇒ 登记为空（触发空转告警）；
    ⑨ T-041 举例段 ⇒ 剔除后**不告警**；⑩ T-041 **真引用不被误剔** ⇒ 仍告警；⑪ 剔除清单可读且 ≥6 层（**不静默**）。
    ★ 以下八侧为 **2026-09-21（v0.309 轮）「假阳性／假阴性同批修」** 新增：
    ⑫ **假阳性**：`GPT-6` ⇒ **引用集不含** `T-6`（反例；**左边界生效**；★ 同侧含双侧对照）
    ⑬ **假阴性（分条行 C）**：报告"谈论 T-6"的分条行 ⇒ **不入登记集**（反例）
    ⑭ **假阴性（节标题 D）**：标题里"谈论 T-6" ⇒ **不入登记集**（反例）
    ⑮ 形态 C **真登记不被误伤** ⇒ **入登记集**（反例；防"收紧过头"）
    ⑯ 形态 D **真登记不被误伤** ⇒ **入登记集**（反例）
    ⑰ **第 6 层生效**：报告体表格行含「本轮告警」⇒ **被剔**（正例）
    ⑱ **第 6 层不误剔**：真引用行不含第 6 层标记 ⇒ **不被剔**、仍告警（反例）
    ⑲ **口径声明（KNOWN-LIMIT）**：形态 B 无法按**形状**区分「登记」与「谈论」
       （`| 7 | Rxx 说明 |` 与 `| x | T-6 假阳性 |` **同形**）⇒ 本侧**断言"口径如此"**并**如实打印**；
       ★ **现状无实例**（实测所有 offending 行为 C/D，表格行 `forms=NONE`）⇒ **不假装已消除**。

【★ v0.309 轮修订（三处同批 · 治两形态判据缺陷）】
  **(a) 假阳性**：族正则**未加左边界** ⇒ `GPT-6` 的子串 `T-6` 被当成编号引用
    ⇒ 六个族正则**统一加 `(?<![A-Za-z0-9])` 前断言**（★ 与既有 `(?![A-Za-z0-9])` 尾断言配对）。
  **(b) 假阴性**：登记形态 **C（分条列表）/ D（节标题）** 过宽（编号出现在行的**任意位置**即算登记）
    ⇒ 报告/台账里「**谈论某编号**」的行被误判为「登记」⇒ **告警自行消失**（T 登记 43 → 44 的实测）。
    ⇒ **C 收紧**为「编号须**紧邻行首分条标记**」；**D 收紧**为「编号须在标题**前段**（允许 `12.1 `／`★` 前缀）」。
    ⇒ ★ **注意**：原判断「命中表格第 2 列（形态 B）」**经诊断证伪** —— 实测 offending 行为 **C/D**，
      表格行 `forms=NONE`（成因是**我当时未读源码就下了归因**，属 R82 家族）。
  **(c) 剔除层扩容**：`scan_exclude.json` 新增**第 6 层**「报告自述告警段」（**表格行／分条行**，带 `line_prefixes`）。
    ⇒ 由 `apply_exclude` 统一实现 —— **层可自带 `line_prefixes`，其 marks 只在行以上述前缀开头时生效**。
"""
import io
import os
import re
import sys
import argparse

# ---- 族定义：编号正则 + 权威登记载体（相对仓库根，仅作"主要落点"提示）----
FAMILIES = {
    # ★ 族正则**统一加前断言 `(?<![A-Za-z0-9])`**（2026-09-21 · v0.309）——
    #   治「假阳性」：`GPT-6` 的子串 `T-6` 曾被打成编号引用。
    # ★ 并保留**尾断言 `(?![A-Za-z0-9])`** —— 排除「模式写法」`T-00N`／`TERM-0NN`／`TERM-00N`
    #   （否则 `T-\d+` 会在 `T-00N` 上截出假编号 `T-00`）。
    "R":    {"pat": r"(?<![A-Za-z0-9])R\d+(?![A-Za-z0-9])",     "auth": ["coordination/BASELINE.md"]},
    "D":    {"pat": r"(?<![A-Za-z0-9])D\d+(?![A-Za-z0-9])",     "auth": ["coordination/BASELINE.md"]},
    "C":    {"pat": r"(?<![A-Za-z0-9])C\d+(?![A-Za-z0-9])",     "auth": ["coordination/CONSTRAINTS.md", "coordination/CHARTER.md"]},
    "T":    {"pat": r"(?<![A-Za-z0-9])T-\d+(?![A-Za-z0-9])",    "auth": ["coordination/TEMPLATES.md"]},
    "TERM": {"pat": r"(?<![A-Za-z0-9])TERM-\d+(?![A-Za-z0-9])", "auth": ["coordination/TERMS.md"]},
    # ★ MECH（机制编号）族 —— 首格为**纯数字**（无族名）⇒ **必须区段限定**（`special: "mech"`，见 `mech_seg_lines()`）。
    #   引用侧用「机制 N」字面（自带族名、全仓无歧义）；登记侧用 `CHARTER.md`「## 三、全部机制」区段内表格首格裸数字。
    #   ★ 2026-09-21 裁定（选项 B）：**区段限定专用解析**启用
    #     依据：`coordination/reports/2026-09-21_MECH族专用解析评估稿.md`（选项 B；备「解析到 0 行 ⇒ 告警」遵 C-04/R83）。
    "MECH": {"pat": r"机制\s*(\d+)(?![0-9])", "auth": ["coordination/CHARTER.md"], "special": "mech"},
}

SKIP_DIRS = {".git", "target", "node_modules", "__pycache__", ".workbuddy"}


def forms(pat):
    """四形态正则（行首锚）。返回 [(form, compiled)]

    ★ 2026-09-21（v0.309）收紧 C / D —— 治「假阴性」：
       旧 C = `^\\s*[-*]\\s*\\*\\*[^\\n]*?ID`  ⇒ 分条行里**任意位置**出现编号即算登记
       旧 D = `^#{2,6}\\s*.*?ID`              ⇒ 标题里**任意位置**出现编号即算登记
       ⇒ 报告/台账里「**谈论某编号**」的行被误判为「登记」⇒ **告警自行消失**（实测 T 登记 43 → 44）。
    """
    idp = "(?P<id>%s)" % pat
    return [
        ("A", re.compile(r"^\|\s*[\*`]{0,4}" + idp + r"[\*`]{0,4}\s*\|")),  # 表格首格行（★ `**` 或反引号包裹均可省 —— 实测三写法并存）
        ("B", re.compile(r"^\|[^|]*\|\s*" + idp)),                     # 表格第 2 列起
        # ★ C 收紧：编号须**紧邻行首分条标记**（可选粗体/反引号包裹）
        ("C", re.compile(r"^\s*[-*]\s*[\*`]{0,4}" + idp)),
        # ★ D 收紧：编号须在标题**前段**（允许 `12.1 ` 段号前缀与 `★` 引导）
        ("D", re.compile(r"^#{2,6}\s*(?:\d+(?:\.\d+)*\s*)?(?:★\s*)?[\*`]{0,4}" + idp)),
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


# ---------------- 告警扫描范围限定：剔除段（★ T-041 落地）----------------
# 规则：扫描前，先剔除 5 层文本（元标注块／待裁定段／问答段／举例引用段／讨论段）。
#   ⇒ 治「自指污染」（判据读到"讨论其自身结论的文字" ⇒ 告警数随报告漂移）。
#   ★ 三条关键约束（防"隐藏规则"，R73 教训）：
#     ① 清单入**机器可读 json**（`scan_exclude.json`，与元标注块同源）；
#     ② 每次扫描**打印「已剔除段落」**（可见）；
#     ③ **剔除规则本身是正式规则**（`TEMPLATES.md` §十八 · T-041）。
#   ★ **只剔引用集，不剔登记集** —— 登记是显式编号行；剔了会造假告警。
EXCLUDE_JSON = os.path.join(os.path.dirname(os.path.abspath(__file__)), "scan_exclude.json")


def load_exclude():
    """读剔除清单（json）。失败 ⇒ 返回 ([], 错误信息) —— **不静默**。"""
    try:
        import json
        with io.open(EXCLUDE_JSON, encoding="utf-8") as f:
            data = json.load(f)
        return data.get("layers", []), None
    except Exception as e:                                  # noqa: BLE001
        return [], "读 %s 失败：%s" % (os.path.basename(EXCLUDE_JSON), e)


def reg_section_exclude_keys():
    """读 `scan_exclude.json` 的 `reg_section_excludes`（**登记侧**排除区段标题关键词）。失败 ⇒ []（不静默：主流程会打印）。"""
    try:
        import json
        with io.open(EXCLUDE_JSON, encoding="utf-8") as f:
            return json.load(f).get("reg_section_excludes", []) or []
    except Exception:                                       # noqa: BLE001
        return []


def reg_excluded_indexes(lines, keys):
    """返回**登记侧**应跳过的行序号集合（0-based）。

    规则：**标题行**含某 key ⇒ 从该标题起、到**下一个标题行**（不含）为止，整段不作登记候选。
    ★ 为什么需要：`A2 豁免登记表` 的首格**就是编号本身**（`| **C22** | … |`）⇒ 会被 A 形态判成
      "该编号已登记" ⇒ **告警一登记就灭灯**（且与 MECH 族"区段限定"口径不一致）。
    ★ 方向是**更严**（让告警保留可见），**不是放宽**。
    """
    out, active = set(), False
    for i, l in enumerate(lines):
        if l.startswith("#"):
            active = any(k in l for k in keys)
        if active:
            out.add(i)
    return out


def apply_exclude(lines, layers):
    """按层剔除行。返回 (kept_lines, drops)；drops＝[(layer_id, layer_name, lineno, text)]。

    ★ 2026-09-21（v0.309）**层可自带 `line_prefixes`**：
        该层的 marks **只在「行以上述前缀开头」时生效**（用于把剔除限定在表格行／分条行，避免误剔正文）。
        缺省（无该键）＝ 不限前缀，行为与旧版一致。
    """
    marks = []
    for L in layers:
        prefixes = L.get("line_prefixes")          # None ⇒ 不限前缀
        for m in L.get("marks", []):
            marks.append((m, L.get("id"), L.get("name"), prefixes))
    kept, drops = [], []
    for i, l in enumerate(lines, 1):
        hit = None
        for m, lid, lname, prefixes in marks:
            if prefixes and not any(l.lstrip().startswith(p) for p in prefixes):
                continue
            if m in l:
                hit = (lid, lname)
                break
        if hit is None:
            kept.append(l)
        else:
            drops.append((hit[0], hit[1], i, l.strip()[:100]))
    return kept, drops


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

    # ==== T-041（告警扫描范围限定 · 剔除层）====
    layers, ex_err = load_exclude()
    _seg = ["| 5 | 机制五 |"]

    # 侧⑨ 正例(T-041)：**举例段**（含「可约定／示例／之类」）⇒ 剔除后 **不告警**
    _l9 = ['- 可约定示例编号一律用 "机制 99" 之类保留号段']
    _kept9, _d9 = apply_exclude(_l9, layers)
    _diff9 = scan_mech_ref(_kept9) - scan_mech_seg(_seg)
    ok = (99 not in _diff9) and (len(_d9) == 1)
    print("  侧⑨（正例·T-041 举例段被剔 ⇒ 不告警）: %s  剔除 %d 行" % ("PASS" if ok else "FAIL", len(_d9)))
    if not ok:
        fails.append("侧⑨")

    # 侧⑩ 反例(T-041)：**真引用**（无剔除标记）⇒ **仍必须告警**（防"剔除过宽"）
    _l10 = ["- 【真缺口】机制 99 尚未登记，需补"]
    _kept10, _d10 = apply_exclude(_l10, layers)
    _diff10 = scan_mech_ref(_kept10) - scan_mech_seg(_seg)
    ok = (99 in _diff10) and (len(_d10) == 0)
    print("  侧⑩（反例·T-041 真引用不被误剔 ⇒ 仍告警）: %s" % ("PASS" if ok else "FAIL"))
    if not ok:
        fails.append("侧⑩")

    # 侧⑪ 对照·**清单可见可读**（"不静默"）：json 必须可读且层数 ≥6（★ v0.309 由 5 扩容为 6）
    ok = (ex_err is None) and (len(layers) >= 6)
    print("  侧⑪（对照·T-041 剔除清单可读且 ≥6 层）: %s  层数=%d err=%s" % ("PASS" if ok else "FAIL", len(layers), ex_err))
    if not ok:
        fails.append("侧⑪")

    # ==== ★ v0.309 轮新增：假阳性 / 假阴性 / 剔除层扩容（七侧 + 1 侧口径声明）====
    _tpat = FAMILIES["T"]["pat"]

    # 侧⑫ 假阳性（**双侧对照**）：`GPT-6` ⇒ **不得**入引用集；而真写法 `T-6` ⇒ **必须**入引用集
    _r12a = scan(["- 见 GPT-6（Astra）说明"], _tpat, reg=False)
    _r12b = scan(["- 见 T-6 说明"], _tpat, reg=False)
    ok = ("T-6" not in _r12a) and ("T-6" in _r12b)
    print("  侧⑫（假阳性·左边界：GPT-6 不命中／T-6 命中）: %s  GPT-6集=%s T-6集=%s" % (
        "PASS" if ok else "FAIL", sorted(_r12a), sorted(_r12b)))
    if not ok:
        fails.append("侧⑫")

    # 侧⑬ 假阴性（分条行 C）：报告"谈论 T-6"的分条行 ⇒ **不得**入登记集
    _reg13 = scan(["- **六族告警**：本轮 2 条（`T-6` 假阳性）"], _tpat, reg=True)
    ok = "T-6" not in _reg13
    print("  侧⑬（假阴性·C 收紧：谈论行不入登记）: %s  登记=%s" % ("PASS" if ok else "FAIL", sorted(_reg13)))
    if not ok:
        fails.append("侧⑬")

    # 侧⑭ 假阴性（节标题 D）：标题里"谈论 T-6" ⇒ **不得**入登记集
    _reg14 = scan(["### 决策 1：`T-6` 假阳性如何处置"], _tpat, reg=True)
    ok = "T-6" not in _reg14
    print("  侧⑭（假阴性·D 收紧：标题谈论不入登记）: %s  登记=%s" % ("PASS" if ok else "FAIL", sorted(_reg14)))
    if not ok:
        fails.append("侧⑭")

    # 侧⑮ 收紧**不过头**（C 真登记）：`- **T-041 ★（…）**：…` ⇒ **必须**入登记集
    _reg15 = scan(["- **T-041 ★（本轮）**：规则说明"], _tpat, reg=True)
    ok = "T-041" in _reg15
    print("  侧⑮（对照·C 真登记不被误伤）: %s  登记=%s" % ("PASS" if ok else "FAIL", sorted(_reg15)))
    if not ok:
        fails.append("侧⑮")

    # 侧⑯ 收紧**不过头**（D 真登记）：`### 12.1 T-041（本轮新增）` ⇒ **必须**入登记集
    _reg16 = scan(["### 12.1 T-041（本轮新增）"], _tpat, reg=True)
    ok = "T-041" in _reg16
    print("  侧⑯（对照·D 真登记不被误伤）: %s  登记=%s" % ("PASS" if ok else "FAIL", sorted(_reg16)))
    if not ok:
        fails.append("侧⑯")

    # 侧⑰ 第 6 层生效：报告体**表格行**含「本轮告警」⇒ **被第 6 层剔**
    _l17 = ["| **本轮告警** | 2 条 |"]
    _k17, _d17 = apply_exclude(_l17, layers)
    ok = (len(_d17) == 1) and (_d17[0][0] == 6)
    print("  侧⑰（第 6 层·报告自述告警段被剔）: %s  剔除=%s" % ("PASS" if ok else "FAIL", [(d[0], d[1]) for d in _d17]))
    if not ok:
        fails.append("侧⑰")

    # 侧⑱ 第 6 层**不误剔**：真引用行不含第 6 层标记 ⇒ 不被剔（仍告警）
    _l18 = ["- 【真缺口】机制 99 尚未登记，需补"]
    _k18, _d18 = apply_exclude(_l18, layers)
    ok = (len(_d18) == 0) and (99 in (scan_mech_ref(_k18) - scan_mech_seg(["| 5 | 机制五 |"])))
    print("  侧⑱（第 6 层·真引用不被误剔 ⇒ 仍告警）: %s" % ("PASS" if ok else "FAIL"))
    if not ok:
        fails.append("侧⑱")

    # 侧⑲ 口径声明（**KNOWN-LIMIT**）：形态 B 无法按**形状**区分「登记」与「谈论」
    #   （`| 7 | R99 说明 |` 与 `| x | T-6 假阳性 |` 同形）⇒ **现状无实例**，风险**如实声明**、不假装已消除。
    _reg19 = scan(["| **本轮告警** | T-6 假阳性 |"], _tpat, reg=True)
    ok = "T-6" in _reg19
    print("  侧⑲（口径声明 · KNOWN-LIMIT：形态 B 同形不可分／现状无实例）: %s  登记=%s" % (
        "PASS" if ok else "FAIL", sorted(_reg19)))
    if not ok:
        fails.append("侧⑲")

    # ==== ★ v0.309 轮新增（续）：登记侧**区段排除**（防"豁免登记表把告警灭灯"）====
    _rkeys = reg_section_exclude_keys()

    # 侧⑳ 区段排除**生效**：`| **C99** | … |` 落在「A2 豁免登记表」节内 ⇒ **不入登记集**（⇒ 仍告警）
    _l20 = ["### 47.3 A2 豁免登记表（兜底）", "| **C99** | 告警内容 | 来源 |", "### 47.4 下一节"]
    _skip20 = reg_excluded_indexes(_l20, _rkeys)
    _reg20 = scan([l for i, l in enumerate(_l20) if i not in _skip20], FAMILIES["C"]["pat"], reg=True)
    ok = ("C99" not in _reg20) and (len(_skip20) == 2)
    print("  侧⑳（区段排除·豁免登记表内不算登记 ⇒ 仍告警）: %s  排除 %d 行 登记=%s" % (
        "PASS" if ok else "FAIL", len(_skip20), sorted(_reg20)))
    if not ok:
        fails.append("侧⑳")

    # 侧㉑ 区段排除**不过宽**：同一行放到**普通标题**下 ⇒ **必须**入登记集
    _l21 = ["### 47.9 普通小节（无排除关键词）", "| **C99** | 告警内容 | 来源 |", "### 47.10 下一节"]
    _skip21 = reg_excluded_indexes(_l21, _rkeys)
    _reg21 = scan([l for i, l in enumerate(_l21) if i not in _skip21], FAMILIES["C"]["pat"], reg=True)
    ok = ("C99" in _reg21) and (len(_skip21) == 0)
    print("  侧㉑（区段排除不过宽·普通小节内仍算登记）: %s  排除 %d 行 登记=%s" % (
        "PASS" if ok else "FAIL", len(_skip21), sorted(_reg21)))
    if not ok:
        fails.append("侧㉑")

    # 侧㉒ 对照·登记侧排除清单**可见可读**（不静默）：配置非空
    ok = (len(_rkeys) >= 1)
    print("  侧㉒（对照·登记侧排除清单非空且可见）: %s  关键词=%s" % ("PASS" if ok else "FAIL", _rkeys))
    if not ok:
        fails.append("侧㉒")

    print("=" * 60)
    if fails:
        print("自检结论：FAIL %d 项 %s" % (len(fails), fails))
        return 1
    print("自检结论：PASS（二十二侧：R 五族 1 正 + 3 反 ＋ MECH 3 反 + 1 空转 ＋ T-041 3 正反 + 1 对照"
          " ＋ v0.309 假阳性/假阴性 5 正反 + 2 层 6 + 1 KNOWN-LIMIT ＋ 登记侧区段排除 2 正反 + 1 对照）")
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
    print("登记口径：A 表格首格行 ∪ B 表格第2列 ∪ C 分条列表〔★ 编号紧邻行首〕 ∪ D 节标题〔★ 编号在标题前段〕")
    print("族正则：六族**统一**加前断言 (?<![A-Za-z0-9])（v0.309 · 治 GPT-6→T-6 假阳性）")

    # ---- ★ T-041「告警扫描范围限定」：剔除 5 层（**只剔引用集，不剔登记集**）----
    layers, ex_err = load_exclude()
    if ex_err:
        print("\n⚠️ **剔除清单读取失败**：%s" % ex_err)
        print("   ⇒ 本次**未剔除**（**不静默**；请检查 `coordination/tools/scan_exclude.json`）")
        ref_lines, drops = all_lines, []
    else:
        ref_lines, drops = apply_exclude(all_lines, layers)
        print("\n【已剔除段落 · T-041】清单＝`coordination/tools/scan_exclude.json`（%d 层）" % len(layers))
        print("   共剔除 **%d 行**（引用集 %d → **%d 行**）；★ **登记集不剔除**" % (
            len(drops), len(all_lines), len(ref_lines)))
        _by = {}
        for lid, lname, ln, tx in drops:
            _by.setdefault((lid, lname), []).append((ln, tx))
        for (lid, lname), items in sorted(_by.items()):
            print("   L%d %s：剔除 %d 行｜示例 L%d：%s" % (lid, lname, len(items), items[0][0], items[0][1][:58]))

    # ---- ★ 登记侧**区段排除**（v0.309 · 防"豁免登记表把告警灭灯"）----
    _rkeys = reg_section_exclude_keys()
    _rskip = reg_excluded_indexes(all_lines, _rkeys)
    reg_lines = [l for i, l in enumerate(all_lines) if i not in _rskip]
    print("\n【登记侧区段排除 · v0.309】关键词＝%s ⇒ 排除 **%d 行**（登记候选 %d → %d 行）"
          % (_rkeys if _rkeys else "（空！配置缺失）", len(_rskip), len(all_lines), len(reg_lines)))
    print("   理由：`A2 豁免登记表` 首格就是编号本身 ⇒ 不排除则「一登记就灭灯」（与 MECH 区段限定不一致）")

    total = 0
    for fam, cfg in FAMILIES.items():
        if cfg.get("special") == "mech":
            # ★ MECH 族：**区段限定专用解析**（选项 B · 2026-09-21 裁定）
            seg = mech_seg_lines(os.path.join(repo, "coordination/CHARTER.md"))
            reg = scan_mech_seg(seg)
            ref = scan_mech_ref(ref_lines)
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
        ref = scan(ref_lines, cfg["pat"], reg=False)      # ★ 引用集：用**剔除后**的行（T-041）
        reg = scan(reg_lines, cfg["pat"], reg=True)       # ★ 登记集：用**全部行 − 登记侧排除区段**（v0.309）
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
