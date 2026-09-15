# WebGPU 宿主 · UI 方案抉择与实施计划（HOST_UI_DECISION）v1.1

> 状态：**v1.0（2026-09-15）** ｜ 依据：发起人「第三阶段：WebGPU 宿主」§三（UI 渲染方式需我方判断）
> 结论：**选 `egui + wgpu`**；但本轮**未交付可运行的窗口宿主**，原因与下一步见 §3/§4（诚实说明）。

---

## 1. 三方案评估（发起人给定选项）

| 维度 | **egui + wgpu**（✅ 选它） | 自绘 2D UI | HTML 叠加层（WebView 做壳） |
|---|---|---|---|
| 与场域管线关系 | **共用同一个 wgpu device/queue**，零组件依赖 | 共用，零新依赖 | **必须再引入 WebView2** |
| 地址栏可行性 | 文本框/光标/选区/**剪贴板/IME** 开箱即用 | 需自造字体图集+光栅化+输入法，工作量以周计 | 天然支持 |
| 与"不依赖系统组件"目标 | ✅ 一致 | ✅ 一致 | ❌ **直接违背根本目标** |
| 内核零依赖红线 | ✅ 依赖只在宿主侧 | ✅ | ❌（且又把 Runtime 请回） |
| 代价 | 宿主 +约 20 个纯 Rust 依赖，exe 变大 | 实现风险高、工期长 | 前功尽弃 |
| 结论 | **选此** | 备选（若日后要求"宿主也零依赖"） | **排除** |

**核心理由**：场域渲染与 UI 共用同一个 GPU 设备，**不引入任何系统渲染组件**；
且"HTML 叠加层"方案等于把刚刚拆掉的 WebView2 装回去，与源头设计目标直接冲突。

---

## 2. 本轮踩到的**真实兼容性障碍**（已查清，下次直接绕开）

加 `egui 0.36.2 + egui-wgpu 0.36.2 + egui-winit 0.36.2` 后：

- `egui-wgpu 0.36` 依赖的是 **wgpu 30**，而本项目管线锁在 **wgpu 23** → Cargo 里同时出现
  `wgpu 23.0.1` 与 `wgpu 30.0.1` 两份；
- 新拉入的 **`wgpu-hal 30.0.1` 的 DX12 后端在本机 GNU 工具链上编译失败**
  （`gpu_allocator` 与 `windows` crate 的 `ID3D12Device` 类型不匹配等错误）；
- **处理**：已**回退**这三个依赖，`field-render` 恢复绿色（构建通过 / 5 单测过 / `--selftest` PASS）。

**两条可选路线（下次择一，需您点一下）**：
- **路线 1（推荐，改动小）**：把 `wgpu` 升到 **30**，与 `egui-wgpu 0.36` 对齐。
  代价：需改少量 API 名（wgpu 24 起 `ImageCopyTexture`→`TexelCopyTextureInfo` 等，wgpu 30 还有
  `Instance::new(&desc)`、`request_adapter → Result`），并**需先确认 wgpu-hal 30 的 DX12 能编译**
  （可先加 `--no-default-features` 只用 Vulkan 后端验证）。
- **路线 2（保守）**：把 egui 降到与 wgpu 23 配套的旧版本（egui 0.29/0.30 系列），继续用 wgpu 23。
  代价：egui 旧版 API 略有差异，但不动已验证的渲染管线。

---

## 3. 本轮为什么没有交付窗口宿主（诚实说明）

我按计划写了宿主草稿，但草稿里为了让 `Host` 持有 device/queue 又不改既有函数签名，
用了 `unsafe transmute_copy` 这类**取巧且危险**的写法。这与本项目"证据驱动、不留隐患"的要求相悖，
**我拒绝把它提交**：宁可这轮少交付，也不把 unsafe 取巧塞进仓库。

按项目规则（"不确定/冲突先停下汇报"），已：
- 删除草稿、**回退依赖**、恢复 `--selftest` 绿色可验证状态；
- 把结论与障碍写成本文件，供下一步直接执行。

**下一步的正确做法**（不再需要探索）：
1. 把 `Gpu` 只当"初始化器"，让 `build_pipeline/upload/encode_frame` 直接吃 `&wgpu::Device/&Queue`
   （纯签名调整，不引入 unsafe）；
2. 宿主结构：`Host { window, surface, config, device, queue, egui_*, pipes: Vec<Pipeline>, tabs, quad }`；
3. 一帧：**同一个 encoder** 内先跑场域 render pass（Clear），再 `forget_lifetime()` 开 egui pass 叠加 UI；
4. tabs：`Vec<Tab { url, source, origin, field, elements, dirty }>`；地址栏用 `ui.text_edit_singleline`；
5. 下载（初版）：`保存场域快照` → 写 `.ppm`（图像，零依赖）+ `.json`（字段与 Gabor 参数）到部署目录，并列出记录；
6. 内核实时驱动：`parse_source → field_to_gabor_with → modulate_gabor(quad)`，脏标记触发重算 1024 个元素。

---

## 4. 现状与验收对照（第三阶段六项验收）

| 验收项 | 现状 |
|---|---|
| 1 窗口能打开并看到场域画面 | ⏳ **未交付**（见 §3） |
| 2 地址栏可输入 URL | ⏳ 未交付（方案已定：egui 文本框） |
| 3 多标签可切换 | ⏳ 未交付（方案已定） |
| 4 下载能触发 | ⏳ 未交付（方案已定：写 .ppm + .json） |
| 5 帧率 > 30 FPS（1024 splat） | ✅ **已验证 312 FPS**（离屏管线，同一套 render pass） |
| 6 无 WebView2 依赖 | ✅ **已验证**（`llvm-objdump -p` 检 WebView2 = 0 处） |

> 说明：5/6 已经在第二阶段用**同一套 wgpu 管线**实测通过；窗口宿主只是把它接到 surface 上。

---

## 5. 版本记录

- **v1.1（2026-09-15 · 第一步完成）**：窗口宿主落地（winit+surface+场域管线，零 unsafe），
  四项验收全过（见 §6）；修掉 2048 纹理上限 panic。
- **v1.0（2026-09-15）**：UI 方案抉择（egui + wgpu，含三方案对比与理由）；
  记录 egui 0.36 ↔ wgpu 23 的**版本冲突实证**与两条解决路线；
  说明本轮未交付窗口宿主的原因（拒绝提交 unsafe 取巧代码）与**可直接执行的六步实施计划**。

---

## 6. 第三步**第一步已完成**：窗口宿主（不含 UI）

发起人改分步推进：**第一步只做「winit 开窗 + wgpu surface + 场域渲染管线接入」**，UI 裁决后置。已完成：

### 6.1 做法（无 unsafe）
- 把 `build_pipeline` / `upload` / `encode_frame` 由 `&Gpu` 改为 **`&Device` / `&Queue`（+ `target_format`）**，
  使**离屏自检与窗口宿主共用同一套管线**（纯签名调整，不复制管线、不取巧）。
- `host/field-render/src/window.rs`：winit 0.30 `ApplicationHandler`（`resumed` / `window_event`）+
  wgpu surface（`create_surface` 在 wgpu 23 是**安全函数**，已核 `api/instance.rs:276`）+ 持续渲染循环。
- **`wgpu::Texture` 在 23 里不是 `Clone`** → 回读时**借用**目标纹理（surface 交换链图像 / 状态持有的离屏纹理），
  不使用 `clone()`、不使用 `transmute`。**全 crate `unsafe` 真实出现次数 = 0**（仅注释里提到）。

### 6.2 客观验收（`--frames N` 自动取证，无需人眼）
| 验收项 | 标准 | 实测 |
|---|---|---|
| 窗口能打开 | — | ✅ **1536×1536**（HiDPI，768 逻辑 × 2）；surface `Bgra8UnormSrgb`；呈现 `Fifo`；`COPY_SRC=true` |
| 能看到场域画面 | 方差 > 1 且非纯黑/非纯白 | ✅ 三样本方差 **43.9 / 1182.8 / 1370.7**，均 `非纯黑非纯白=true` |
| 帧率 | > 30 FPS | ✅ **呈现口径 60–62 FPS**（垂直同步上限）；**管线裸口径 115–554 FPS**（1024 splat） |
| 无 WebView2 依赖 | 无 `WebView2Loader.dll` | ✅ `llvm-objdump -p` 检 **WebView2 = 0 处** |

**回读来源＝`surface`（真实上屏的那张纹理）**——不是离屏近似，故方差证据可信。
pHash 汉明距离（真实上屏纹理间）：文本↔图片 **30**｜文本↔图片·紧张 **32**｜图片↔图片·紧张 **6**（阈值 >10 ✅）。

### 6.3 本轮修掉一个真实缺陷
`wgpu::Limits::downlevel_defaults()` 把 `max_texture_dimension_2d` 限在 **2048**，而 HiDPI 下窗口可达 **2134** →
`Surface::configure` 直接 Validation Error **panic**。修法：设备用
`downlevel_defaults().using_resolution(adapter.limits())`，并在建窗与 `resize` 两处把尺寸**钳制**到设备上限。

### 6.4 仍未做（如实）
地址栏 / 多标签 / 下载 / egui —— **属第三步后续步骤**，等 UI 路线裁决（§1 三方案）。
