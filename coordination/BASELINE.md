# 云内核项目 · 基准

最后更新于：HEAD 7a42407（对应 v0.135）｜本轮（机制简化 + D10–D13 执行）更新内容见下方（本次更新随提交计入下一版）
更新时间：2026-09-16

> **口径说明**：上面一行记录的是「**本文件最后一次更新时所处的 HEAD / 版本**」，**不是「当前版本」**。
> 本文件的内容（指标、状态）反映的是那次更新时的事实快照，会随提交而自然滞后。
> 「当前版本」请一律以 `git rev-list --count HEAD` 为准（本项目版本号自动生成、不手写，见 CONSTRAINTS.md C2）。

## 〇、项目根（唯一权威）

| 项 | 值 |
|---|---|
| **唯一权威项目根** | `C:/Users/香忆/WorkBuddy/研发开发/tmp_a45_review/gh_clone2` |
| 版本号来源 | 上述仓库的 `git rev-list --count HEAD` |
| 陈旧分叉归档位置 | `C:/Users/香忆/WorkBuddy/研发开发/archive/` —— 内含 `meta-kernel-v0.48/`（自带 git，仍可自证 v0.48 / `cbf1b1d` / 74 处未提交改动原样封存）与 `meta-kernel_repair-0907/`（更早的修复副本，自带 git）。**均为移出根目录、保留不删** |
| 其它同名/相似目录 | `研发开发/` 本身**不是** git 仓库；根目录下已无陈旧分叉 |

> **判据**：任何"当前版本/提交/改动"的统计，一律以上表仓库为准；发现别处也有 `meta-kernel-core/` 时，**默认它是陈旧副本**。
> **D1-a 已执行**：`研发开发/meta-kernel/` → `archive/meta-kernel-v0.48/`（移动前后均 240 个文件，无删除）。
> **D13 已执行**：`研发开发/meta-kernel_repair_0907/` → `archive/meta-kernel_repair-0907/`（移动前后均 100 个文件，无删除）。
> ⇒ `研发开发/` 根目录**已无任何陈旧分叉**。

## 一、项目定位

在现有操作系统之上生长的"意识补充层"，不替代、不淘汰。
让每个设备、每个系统、每个界面都获得"生命感"。

## 二、命名体系

| 版本 | 系统名 | 内核层 | 操作系统层 | 浏览器层 | 大模型 |
|---|---|---|---|---|---|
| 电信号 | 元内核系统 | 基因内核 | 云操作系统 | 空天浏览器 | 云海大模型 |
| 量子 | 量子云系统 | 量子钟 | 正源操作系统 | 正源浏览器 | 采微大模型 |

输入法：彩虹输入法（统一）

## 三、层级架构

| 层级 | 名称 | 状态 |
|---|---|---|
| L0 | 0锚点/量子真空 | ✅ |
| L1-L3 | 基因内核（含基因库、学习机制、世界模型） | ✅ |
| L4 | 拒绝层（含四元组风险戒律） | ✅ |
| L5 | 诊断层（含语境、四元组、证据、注意力） | ✅ |
| L6 | 交互层（空天浏览器） | 🚧 |
| L7 | 执行层（多内核互联+修复执行） | 🚧 设计完成 |
| L8-L10 | 传承/反哺/自明 | 🔒 冻结 |

## 四、已完成

**内核**：三引擎、气泡沙漏、镜像池、摩尼宝珠、痕迹/习气/自我、正源场域、基因库四层、学习机制、场域解析器、场域映射库、世界模型、七维向量、不非时食/不捉持、风险戒律四项、四场拆解、本底场、语境模块、亢枯平、诊断结论、多语言翻译、四元组、证据模块、注意力模块、多内核互联、动作分级、T1闭环、动作账。

**宿主**：wry降级、field-render离屏、window.rs窗口、egui UI、双pass复用、动态LOD、一致性映射、gene_store持久化。

**持久化**：基因库、痕迹、习气、四元组。

## 五、未完成

| 项 | 状态 |
|---|---|
| 宿主真实运行验证 | ✅ **已达成**（2026-09-16 本机真实开窗 + 输入 URL + 取源码 + 上屏回读 PASS；见 `reports/2026-09-16_R1宿主真实运行调研.md`） |
| **CI 覆盖宿主（R6）** | ✅ **已落地并已全绿**：`host-windows` job（windows-latest）**全步骤 success**，含 ①②③ 客观断言 **与 ④ 真实开窗**（run 35044362728） |
| **CI 全绿（本轮目标）** | ✅ **达成**（2026-09-16，run `35044362728`：`test` 绿 + `host-windows` 全绿） |
| **机制总纲缺失** | ✅ **已建，并按评审简化为"能跑的最小集"**：`CHARTER.md`＝**自包含版**（前八节只读它就有全图；新增 **§五 启动回执**、**§六 动作约束 C10**、§七 紧急通道、§八 冲突解决）；活相机制三文件**合并为一个**（`TEMPLATES.md` §七「变更记录」，原 `TEMPLATES_CHANGELOG.md` / `OBSERVATIONS.md` 已删除）；`README.md` 改为极简入口 |
| **C9 门禁无 CI 断言（R10 / D11）** | ✅ **已补**：CI `host-windows` **验收⑤**——`transmute` 零出现 + 真实 unsafe 必须全在白名单 + `// SAFETY:` 覆盖数 ≥ unsafe 处数 + **门禁自检（正反两侧样例）**；本地原样干跑 **exit=0** |
| **CI 验收④为诊断性（R11 / D10）** | ✅ **已升为门禁**：去掉 `continue-on-error`，改为硬断言（退出码 0 + 回读来源＝surface + 结论 PASS） |
| **C10 动作约束缺失** | ✅ **已增补**：`CONSTRAINTS.md` C10 + `CHARTER.md` §六；须用户确认的动作清单与判据见正文（本行为机制登记，不含任何实际操作） |
| **CI 宿主 job 链接失败（R1-b）** | ✅ **已修**：libgcc **语义映射**（`libgcc.a←compiler-rt builtins`、`libgcc_eh.a←libunwind`）；本机在官方 20260908 干净包上实测 exit=0 |
| **`--diag-check` 断言（R7）** | ✅ **已修**（断言对齐中性点 m=0.5；本机 PASS） |
| **meta-kernel/ 陈旧分叉（R3/D1-a）** | ✅ **已归档**（→ `研发开发/archive/meta-kernel-v0.48/`） |
| 老笔记本实测 | ⏳ 待用户 |
| L5诊断端到端（宿主运行期字段级） | ⏳ 部分 |
| 推送SNI摩擦根因 | ❌ 未定位 |
| `--quad-check` 喜欢维 | ⚠️ PARTIAL（变化 0.5%，已知待加强；**须在内核映射层改，禁止宿主调参打绿**） |
| 阶段二（可引导 ISO） | 🚧 子任务1 **已收尾**（基础修复 + 机制落地：CHARTER／模板固定相／CI 宿主 job 修复）；子任务2（mkosi 环境搭建）待启动 |
| L6感知、L7执行器 | ⏳ P2 |
| 覆盖率、知识库 | ⏳ P2 |

## 六、已知漏洞/风险

| 编号 | 漏洞 | 状态 |
|---|---|---|
| R1 | 宿主从未真实端到端运行 | ✅ **已解除**（2026-09-16 本机真实运行 PASS） |
| R2 | 推送SNI摩擦反复出现 | 🟡 未定位（本机连续两轮 push 均一次成功，疑偶发） |
| R3 | meta-kernel/ 陈旧分叉 | ✅ **已解除**（归档至 `研发开发/archive/meta-kernel-v0.48/`，保留不删） |
| R4 | ~~本地缺 dlltool，宿主无法本地链接~~ **实为 PATH 未含 llvm-mingw `bin`** | ✅ **已澄清并绕过**，且**已固化为 CI 步骤**（勿删） |
| R5 | 二进制banner显示CARGO_PKG_VERSION而非commit count | 🟢 待评估 |
| R6 | **CI 从不构建宿主** → 宿主验收无自动回归 | ✅ **已解除**（新增 `windows-latest` job：GNU+llvm-mingw 构建 + 4 项客观断言） |
| R7 | `--diag-check` 子项①断言与 `m=0.5` 中性点不符 | ✅ **已修**（改为按 (2m−1) 符号预测方向 + 新增 m<0.5 探针强制覆盖；本机 PASS） |
| R8 | `--quad-check` 喜欢维变化仅 0.5%（阈值 >10%） | 🟡 已知待加强（**不许在宿主层调参打绿**） |
| R9 | 宿主运行期产物写到 CWD 易误入库 | ✅ **已修**（`.gitignore` 增 `gene_library.txt`/`trace_store.txt`/`habit_pool.txt`/`quad_state.txt` 及 `.bak`） |
| R10 | C9 门禁曾**只有文字约定、无 CI 断言** | ✅ **已修**（D11 执行：验收⑤落地 = `transmute` 零出现 + 白名单子集 + `// SAFETY:` 覆盖数 + **门禁自检**） |
| R11 | CI 的「真实开窗」步骤曾为**诊断性**（`continue-on-error`） | ✅ **已升为门禁**（D10 执行）；升级依据＝run `35044362728` 上 ④ **真实通过**（开窗 1080×760 · Dx12(WARP) · 取源码 · **surface 回读** · PASS · exit=0） |
| **R16** | **C9 门禁首跑出现假阳性**：初版判据用 `grep -rn "unsafe"`（无词边界）→ 把 `let unsafe_q = ...`（**变量名**）与注释里提到的 `transmute` 都判成违规（本地干跑即被拦下） | ✅ **已修**：判据锚在**关键字/调用的确切用法**上（只认 `unsafe{` / `unsafe fn` / `impl` / `trait` / `extern` 与 `transmute` 调用）+ 剔除整行注释；并把该案例**固化为门禁自检回归样例**（正反两侧，防止将来放宽正则时又漏回去） |
| **R17** | **宿主自检的帧率门线没有环境感知**：`window.rs::finish()` 硬编码 `present_fps > 30 || raw_fps > 30`。CI runner 无 GPU、落到 Dx12 **WARP**（自报 `IntegratedGpu`）时只有 ~14 FPS ⇒ 宿主自身返回 **FAIL / exit=1**。在"诊断性步骤"时代被 `continue-on-error` 掩盖；**升为门禁后立刻暴露**（run `35067319775`：链路全通——开窗/取源码/surface 回读/方差 100.3 全 PASS，唯独帧率门线把它判死） | ✅ **已修**：新增 `adapter_is_software()`（关键词表与 CI 逐字一致，**名字优先**——WARP 自报 IntegratedGpu）+ `finish()` 改**环境感知**：**硬件适配器门槛一字未改（≥30）**，软件适配器只要求 >0 并如实标注。附 3 条单元测试（正反两侧 + Cpu 类型）。⚠️ **这不是"把 30 调低"** |
| **R18** | **`field-render` 有 3 个单元测试长期必红**（本地 `cargo test` 恒 FAILED：`rotation_keeps_previous_as_bak`、`corrupt_main_falls_back_to_bak`、`corrupt_traces_falls_back_to_bak`）——CI 的 `test` job 只跑 workspace（**不含**这个独立 crate），故从未被人发现 | ✅ **已修**（**测试自身缺陷，非产品缺陷**）：① `rotation_*` 把期望值硬编码为 `1.618`（3 位小数），而实际种子值是 `GOLDEN_HIGH`（黄金比例 1.6180339887…），容差 1e-12 ⇒ 必然失败 → 改用权威常量；②③ 两个 `corrupt_*` 只 save 一次，而轮转语义是"主文件已存在时才复制为 `.bak`" ⇒ 根本不会产生 `.bak` → 各补一次 save。**产品语义未改** |
| **R12** | **本地 llvm-mingw sysroot 里存在「伪装库」**：`libgcc.a`/`libgcc_eh.a` 实为 `libunwind.a` 的拷贝（两文件 md5 相同、非官方包内容，时间戳 Sep 8）——本机长期"能构建"建立在此 hack 上，**不可复现**，且命名与内容不符会误导诊断 | ✅ **已查明并修正**（2026-09-16）：本机 sysroot 已改为**语义映射**（`libgcc.a←compiler-rt builtins`、`libgcc_eh.a←libunwind`），原文件留痕为 `*.hackbak`；CI 同步采用该映射 |
| **R13** | CI 断言步骤存在**诊断盲区**：GitHub 的 `shell: bash` 外层自带 `-e`，步骤内 `set -uo pipefail` **去不掉它** → 被测程序非零退出即终止脚本，`CODE=$?`/`cat 日志` 被跳过，只见 exit 码不见原因 | ✅ **已修**（验收①/②/③ 显式 `set +e` + `RUST_BACKTRACE=1`；与验收④对齐） |
| **R14** | **Dx12 后端下宿主根本起不来**（真实缺陷，非 CI 环境问题）：`sort.wgsl` 的 `Splat2D.cov2d: mat2x2<f32>` 经 naga 的 **HLSL 后端**在"整结构体赋值"降级时生成的代码被 FXC 拒绝 —— `error X3018: invalid subscript 'cov2d'`（`Device::create_compute_pipeline` → panic 101）。本机走 Vulkan 不经 FXC，故长期未暴露；**Windows 用户默认后端即 Dx12** | ✅ **已修**：`cov2d` 改为 `array<vec2<f32>, 2>`（WGSL 布局完全相同，64 字节不变）。**Dx12 与 Vulkan 双后端本机实测均 exit=0 且 PASS，方差/pHash 逐位一致** |

## 七、关键指标

| 指标 | 数值 |
|---|---|
| 宿主是否真实运行 | ✅ **是**（2026-09-16，本机 Intel Arc / Vulkan，窗口 2160×1520，URL 取源码成功，回读自 surface） |
| 离屏帧率（本机 debug） | **191.8–209.3 FPS**（1024 splat）｜本轮本机实测方差 84.0 / 600.2 / 876.8（均 > 1） |
| 窗口帧率 | 呈现 **59.5 FPS**（垂直同步）｜管线裸口径 **355.0 FPS**（debug，256 splat） |
| WebView2依赖 | 0处 |
| unsafe | **真实 unsafe 关键字 0 处**（`unsafe_whitelist.txt` 为空 ⇒ 未放行任何条目；机制见 C9，**CI 门禁见验收⑤**）。ℹ️ `meta-kernel-core/src/l4_risk.rs` 里的 `unsafe_q` 是**变量名**（"不安全状态的四元组"），不是 unsafe 语法——C9 门禁首跑曾误报它，见 R16 |
| 内核测试 | 411项（lib 390 + 集成 21） |
| 本地验收模式实跑 | **11 个全跑**：**10 PASS**（含修复后的 `diag-check`）｜1 PARTIAL（`quad-check`·已知待加强） |
| CI 覆盖 | ✅ **全绿**（run `35044362728`）＋ 本轮新增**验收⑤**：`test` job（ubuntu）＋ **`host-windows` job（windows-latest）**：装 llvm-mingw → **libgcc 语义映射**（`libgcc.a←compiler-rt builtins`、`libgcc_eh.a←libunwind`）→ GNU 构建宿主 → ①方差>1/帧率 ②排序 ③L5诊断 ④**真实开窗+surface 回读（门禁）** ⑤**C9 门禁（unsafe 白名单 + 禁 transmute + 门禁自检）**；另含「环境诊断」步骤（打印适配器，不计门禁） |
| **CI 上真实运行** | ✅ **是**（run `35044362728` 验收④）：runner 无 GPU，落 **Dx12 WARP 软件适配器**，仍完成 **开窗 → 输入 URL → 取源码（108 字）→ 从 surface（真实上屏纹理）回读 → 结论 PASS**；呈现 20.9 FPS（软件口径，**未按硬件 ≥30 断言**） |
| 验收模式 | 11个：`selftest`／`sortcheck`／`quad-check`／`like-check`／`lod-check`／`world-check`／`diag-check`／`link-check`／`l4-check`／`persist-check`／`ui-selftest` |

## 八、硬约束

全文见 `CONSTRAINTS.md`；一句话速查见 `CHARTER.md` §四。**十条**摘要：
C1 内核零依赖｜C2 版本号自动生成不手写｜C3 不用 unsafe 取巧代码（无例外）｜C4 改前先列清单、确认后执行｜
C5 诚实标注未完成项（本地/CI/真实/未验证）｜C6 每次报告带三量台账｜C7 不用 CI 通过代替真实运行｜
C8 硬约束可扩充（用户确认后生效）｜C9 unsafe 只允许边界形态（边界集中 + `// SAFETY:` + 禁 transmute + 白名单，**已有 CI 门禁**）｜
C10 **动作约束**（凡"外部可见或不可逆"的动作须用户明确确认；清单与判据见 `CONSTRAINTS.md` C10 / `CHARTER.md` §六）

## 九、待用户决策

| 编号 | 事项 | 建议 |
|---|---|---|
| D1 | ~~meta-kernel/ 陈旧改动如何处置~~ | ✅ **已执行**：归档至 `研发开发/archive/meta-kernel-v0.48/`，保留不删 |
| D2 | 推送SNI摩擦是否专项排查 | 本轮 push 未见复发，可后置 |
| D3 | 宿主真实运行路径选择 | ✅ **已定并已执行**：PATH 加 llvm-mingw `bin`（并已固化为 CI 步骤） |
| D6 | ~~是否修 `--diag-check` 断言（R7）~~ | ✅ **已执行**（对齐中性点 m=0.5，本机 PASS） |
| D7 | ~~是否给 CI 加 windows job（R6）~~ | ✅ **已执行**（新增 `host-windows` job） |
| D8 | `quad-check` 喜欢维（R8）排期 | P2；须在**内核映射层**改，禁止宿主调参打绿 |
| D9 | ~~阶段二 unsafe 边界是否放行~~ | ✅ **已执行**：C9 已增补 + 白名单机制就位（当前为空，未放行任何条目） |
| **D10** | CI「真实开窗」何时从**诊断性**升为**门禁**（R11） | ✅ **已执行**：去掉 `continue-on-error`，验收④＝门禁（退出码0 + 回读来源＝surface + 结论 PASS）；依据＝run `35044362728` 真实通过 |
| **D11** | C9 门禁尚无 **CI 断言**落地（R10） | ✅ **已执行**：CI 验收⑤（`transmute` 零出现 + 白名单子集 + `// SAFETY:` 覆盖 + **门禁自检**）；本地原样干跑 exit=0 |
| **D12** | CI FPS 断言在**软件适配器**（runner 无 GPU）下按 >0 而非 ≥30 | ✅ **已定（接受现状）**：如实标注软件口径、不按硬件断言；**不为此自建带 GPU runner**（成本不划算）；若将来需要，按 D12 重开 |
| **D13** | `研发开发/meta-kernel_repair_0907/` 如何处理 | ✅ **已执行**：归档至 `研发开发/archive/meta-kernel_repair-0907/`（100 个文件，移动前后一致，无删除）；根目录已无陈旧分叉 |
| **D14** | CI 的 libgcc 语义映射依赖 llvm-mingw 内部布局（`lib/clang/<ver>/lib/windows/libclang_rt.builtins-x86_64.a` + `x86_64-w64-mingw32/lib/libunwind.a`） | 已在安装步骤内**断言两个映射源存在**（缺源即明确报错）；升大版本时若布局变化，按报错同步即可，不必钉版本 |
| **D15** | 机制是否引入**轻量档**（小任务只带基准/任务/状态/边界说明四项） | 🔬 **观察中**（OBS-002）：至少观察 3 轮再定，不急于加复杂度；**三量台账与边界说明任何时候都不省**（C5/C6） |
| **D16** | `CHARTER.md` 自包含版 / `TEMPLATES.md` §七 变更记录 / C10 动作约束 / 启动回执 —— 本轮四项新机制 | 🔬 **试用中（3 轮）**：本轮为第 1 轮；3 轮后回顾决定"生效 / 调整 / 废弃"（记入 `TEMPLATES.md` §7.2） |
