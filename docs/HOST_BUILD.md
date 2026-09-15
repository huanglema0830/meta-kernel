# 宿主 field-render · 构建与验收手册

> 适用：`host/field-render/`（**独立 crate，不并入 workspace**）。
> 本文件是「宿主怎么建、怎么跑、怎么验收」的**单一事实源**。
> 最后更新于：HEAD 2a4d2dc（对应 v0.129）——快照口径，当前版本见 `git rev-list --count HEAD`。

## 一、构建（Windows / GNU 工具链）——**必须先做这一步**

**把 llvm-mingw 的 `bin` 前置到 `PATH`**：

```bash
export PATH="C:/Users/香忆/WorkBuddy/研发开发/tmp_a45_review/llvm_mingw/llvm-mingw-20260826-ucrt-x86_64/bin:$PATH"
cd host/field-render
cargo build --target-dir C:/c/fr-build2      # 短 target-dir，规避 MAX_PATH
```

### 为什么（v0.126 实测根因，**不是"缺 dlltool"**）

- `windows-sys`（raw-dylib）与 `libloading` 在**链接期需要 `dlltool`** 生成导入库；
  rustc 只在 **`PATH`** 里找它。找不到即报：
  `error calling dlltool 'dlltool.exe': program not found`。
- rustup 自带的 GNU `dlltool`（在 `…/x86_64-pc-windows-gnu/bin/self-contained/`）**没用**：
  它需要汇编器 `as`，**且只在自己所在目录里找 `as`** → `CreateProcess` 失败。
  （实测：把替身 `as.exe` 放进 dlltool **同目录**后，它才成功启动子进程——证明它是"同目录找 as"。）
- **llvm-mingw 的 `dlltool.exe` = `llvm-dlltool`（自包含 LLVM，不需要 `as`）** → 前置到 PATH 即通。
- 本机 `~/.cargo/config.toml` 已把 GNU 链接器指向该工具链的 `x86_64-w64-mingw32-gcc.exe`；
  **但 PATH 没带上 `bin` —— 配置只写了一半**，这就是长期误判为"缺 dlltool"的原因。

### ⚠️ llvm-mingw 版本约束（**勿随意升级**；v0.130 实测）

sysroot **必须自带 `libgcc.a` / `libgcc_eh.a`**：

- 本机在用 **`20260826`**：`x86_64-w64-mingw32/lib/` 含 `libgcc.a`、`libgcc_eh.a`、`libmingw32.a`、`libmingwex.a` ✅
- **`20260908` 起已移除这两只库**，该目录只剩 `libmingw32/wex/thrd.a` ❌
- 驱动（`x86_64-w64-mingw32-gcc.exe`）的库搜索路径**包含自己的 sysroot**：
  `…/lib/clang/23`、`…/x86_64-w64-mingw32/lib`、`…/x86_64-w64-mingw32/mingw/lib`、`…/lib`
  —— 所以**能不能解析 `-lgcc_eh` / `-lgcc`，取决于这个目录里有没有那两只库**。

**为什么会"本机能过、CI 不能过"（关键不对称）：**

| | 本机 | CI `windows-latest` |
|---|---|---|
| rustup 工具链 host | **GNU**（`stable-x86_64-pc-windows-gnu`） | **MSVC**（`dtolnay/rust-toolchain@stable` 默认） |
| gnu target 的 `lib/rustlib/x86_64-pc-windows-gnu/lib/self-contained/` | **44 个文件**，含 `libgcc.a`/`libgcc_eh.a`/`libgcc_s.a` + 全套 mingw 导入库（来自 `rust-mingw` 组件） | **只有 2 个**：`crt2.o`、`dllcrt2.o` |
| llvm-mingw 若不带 libgcc | 仍能从 `self-contained` 兜住 → **假绿** | **无人提供 → `lld: error: unable to find library -lgcc_eh`** |

⇒ 本机用 llvm-mingw `20260908` 也**照样能构建**（被 `self-contained` 掩盖），**只有 CI 会炸**。
CI 里已把这条约束变成**显式门禁**（安装步骤断言 sysroot 存在 `libgcc.a`/`libgcc_eh.a`/`libmingw32.a`/`libmingwex.a`），
升级版本时会**在那一步明确报错**，而不是给一句难懂的 lld 链接失败。

**最小复现（照抄 CI 失败行的库序列，跨版本对照）：**

```bash
echo 'int main(void){return 0;}' > t.c
# 用某版 llvm-mingw 的驱动、按 CI 的库顺序链接
"$LM/bin/x86_64-w64-mingw32-gcc.exe" t.c -o t.exe \
  -Wl,-Bstatic -Wl,-Bdynamic -lkernel32 -lntdll -luserenv -lws2_32 -ldbghelp \
  -lgcc_eh -l:libpthread.a -lmsvcrt -lmingwex -lmingw32 -lgcc -lmsvcrt -lmingwex -luser32 -lkernel32 \
  -nodefaultlibs
# 20260908 → lld: error: unable to find library -lgcc_eh / -lgcc（与 CI 报错逐字一致）
# 20260826 → 那两条错误消失（只剩探针自身缺 Rust crt 符号的无关报错）
```

### MSVC 路径（**本机不可用**，仅记录）

`cargo +stable-x86_64-pc-windows-msvc --target x86_64-pc-windows-msvc` + `-Clinker=lld-link`
能进链接，但**缺 Windows SDK/CRT 库**（kernel32 / ntdll / userenv / ws2_32 / dbghelp / msvcrt）——
本机无 VS2022，故**排除**。（CI 的 `windows-latest` runner 自带 VS2022，是另一回事。）

## 二、类型检查（无需链接，可本机快速迭代）

```bash
cargo check --tests --target-dir C:/c/fr-check   # 比 cargo check 多覆盖测试代码层
```

## 三、真实运行（验收）

```bash
EXE=/c/c/fr-build2/debug/field-render.exe

"$EXE" --selftest                     # 离屏客观自检（GPU 管线 + 排序 + 方差 + 帧率）
"$EXE" --sortcheck                    # GPU 双调排序正确性
"$EXE" --diag-check                   # L5 诊断（证据调制 · 中性点 m=0.5 对齐）
"$EXE" --url http://127.0.0.1:18124/page.html --frames 120   # 真实开窗 + 输入 URL + 上屏回读
```

共 **11 个验收模式**：`--selftest｜--sortcheck｜--quad-check｜--like-check｜--lod-check｜
--world-check｜--diag-check｜--link-check｜--l4-check｜--persist-check｜--ui-selftest`
（另有 `--frames N｜--sample N｜--url U`）。

### 2026-09-16 本机真实运行记录（debug）

```
[window] 窗口 2160x1520 · surface Bgra8UnormSrgb · Fifo · COPY_SRC=true · egui 0.30 + wgpu 23（无 WebView2）
[window] 适配器: Intel(R) Arc(TM) Graphics / Vulkan
[window] 取源码成功：http://127.0.0.1:18124/page.html（412 字）｜四场 地0.37 水0.63 火0.00 风0.00｜置信度 0.57
[window] 帧率：呈现 59.5 FPS｜管线裸口径 355.0 FPS
[window] 回读来源：surface（真实上屏纹理）｜结论：PASS
```

离屏 `--selftest`：**191.8–209.3 FPS**（1024 splat，debug），方差 84.0 / 600.2 / 876.8（均 > 1）。

## 四、断言口径（别把"绿"当"对"）

- **断言必须打在真实语义上**：只测"非纯黑／方差>1"会一直绿（没排序也能出图）。
  跨内容比较用 dHash；**同一内容内的调制必须用像素级度量**（平均绝对差 / 变化像素占比）。
- **断言必须与公式的中性点对齐**（R7 教训）：
  `l5_evidence::adjust` 的 `world_adjust = world_gain × (2m − 1)` ⇒ **中性点 m = 0.5**；
  `pe_adjust = pe_gain × (2s − 1)`，中性点 **s = 0.5**。
  曾把 m=0.585（> 0.5）当"低匹配"而误报 FAIL —— **是脚本缺陷，不是内核缺陷**。
  现改为"按 (2m−1) 的符号预测方向，再与实测比对"，并强制覆盖 m<0.5 分支。
- **绝不靠调参打绿**。

## 五、CI

`.github/workflows/ci.yml` 的 **`host-windows`** job（`windows-latest`）：
构建宿主并执行客观断言 ①方差>1 + 帧率 ②排序 ③L5诊断；④真实开窗为诊断性步骤。
其"安装 llvm-mingw 并把 `bin` 前置到 PATH"步骤是 **R1 修复的固化**，**勿删**；
该步骤同时**钉住版本 `20260826`** 并**断言 sysroot 自带 libgcc**（原因见第一节的版本约束，勿随意升级）。

帧率断言口径：**硬件适配器 → ≥30**；软件适配器（CI runner 无 GPU）→ 如实标注并按 >0 断言
（不拿"改阈值"打绿，也不拿"CI 跑过"冒充硬件口径）。

CI 里**不还原** `self-contained`（MSVC host 没有 `rust-mingw` 组件）——这正是"本机能过 CI 不能过"的分界；
若要彻底消除这条不对称，可改为在 CI 装 **GNU host 工具链**（`rustup toolchain install stable-x86_64-pc-windows-gnu
--component rust-mingw-x86_64-pc-windows-gnu`）后用 `cargo +stable-x86_64-pc-windows-gnu` 构建，此时 libgcc 由
`self-contained` 提供、与 llvm-mingw 版本解耦。**当前未采用**（记为决策项 D14，见 `coordination/BASELINE.md`）。

## 六、运行期产物

宿主运行会在 **CWD** 写出：`gene_library.txt`、`trace_store.txt`、`habit_pool.txt`、`quad_state.txt`（各带 `.bak`）。
均为**运行状态**非源码，已在 `.gitignore` 中排除。
路径可用 `META_KERNEL_GENE_LIB` / `_TRACES` / `_HABITS` / `_QUAD` 覆盖（便于隔离验收）。
