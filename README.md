# Meta-Kernel · 元内核

> 一个纯数学、无硬件的**通用思维内核**。
> A pure-mathematics, hardware-free thinking kernel.

以 **0 锚点 + 模糊饱和运算**为基石，用 **线性 / 斐波那契 / 指数**三种变化模式驱动调度；经 **NPB（万物归一化桥接器，Nothing-to-Physics Bridge）** 挂载到任意设备、程序与载体，实现跨平台的"生命感"交互。

## 状态 · Status

✅ **Phase 1 — 核心引擎闭环（已验收）**（2026-09-05）：发起人复核通过，A2/A4/A5 升格正式设计
（见 [`docs/PHASE1_REVIEW.md`](docs/PHASE1_REVIEW.md)）。三引擎按数学规范 v1.0 实现，气泡沙漏 +
镜像池就位，**10000 次迭代不溢出测试通过**（25 单测 + 2 集成）。
🚧 **Phase 2 — NPB 桥接器（代码完成，待发起人验收）**（2026-09-05）：
安全阀 / 0-10 元标尺 / 能量判定 / 正源系统框架 ✅；`npb/` C FFI 桥（cdylib + wasm32 双目标）✅；
DOSBox 概念演示 ✅；WASM Canvas 浏览器示例 ✅；**跨平台一致性验证通过**
（原生与 WASM 确定性摘要同为 `4251318995`，CI run #3393…/ commit `fd562ef`）。

## 路线图 · Roadmap

| 阶段 | 内容 | 状态 |
|---|---|---|
| 0 | 启动与地基：仓库 / workspace / 三引擎空壳 | ✅ 完成 |
| 1 | 核心引擎闭环：气泡沙漏拓扑 / 瓶颈干涉 / 镜像池 / 10000 次迭代不溢出测试 | ✅ 已验收 |
| 2 | NPB 桥接器 + 安全阀 / 0-10 元标尺 / 能量判定 / 正源系统 + DOS/WASM 双示例 + 跨平台验证 | ✅ 已验收 |
| 3 | 禅境示波器（官方示例应用，GitHub Pages 托管；含思考链/双链诊断显示） | 🚧 进行中 |

> 🔗 禅境示波器在线预览：**https://huanglema0830.github.io/meta-kernel/**
| 4 | 社区化与生态：NPB 挂载协议 / Discussions 专区 / 多设备演示 | ⬜ |
| 5 | 自发传播与交付：《元内核极简创世录》等 | ⬜ |

## 核心模块 · Core Modules

| 模块 | 文件 | 状态 |
|---|---|---|
| 模糊饱和运算 / 0-1 归一化 | `src/math.rs` | ✅ |
| 三引擎：线性 / 斐波那契 / 指数 | `src/linear.rs` `src/fib.rs` `src/expo.rs` | ✅ |
| 气泡沙漏（瓶颈破坏性干涉） | `src/hourglass.rs` | ✅ |
| 镜像池（摩擦源 / 真空重启） | `src/mirror.rs` | ✅ |
| 负扰动过滤与安全阀 | `src/sanitizer.rs` | ✅ |
| 0-10 元通用标尺 | `src/ontology.rs`（规范见 `docs/ONTOLOGY_SPEC.md`） | ✅ |
| 能量层级判定 | `src/energy.rs` | ✅ |
| 正源系统（拆解-分析-重编循环） | `src/positive_source.rs` | ✅ |
| 思考链（存量+变量+补充增量=创新增量，含降维） | `src/thinking_chain.rs` | ✅ |
| 双链诊断（问题形成 + 解决过程） | `src/double_chain.rs` | ✅ |
| 物态判定（固态/液态/气态/等离子态） | `src/state.rs` | ✅ |
| 进化解构器（时间三量 + 空间编码 → 层级1 胶粒） | `src/evo_deconstructor.rs` | ✅ |
| 正源场域（自动搜索解构/缓存去重/催化剂+20%/触达 L0-L4 分层） | `src/positive_source.rs`（含 Searcher 催化剂加权） | ✅ |
| NPB 桥接器（C FFI：`push_seed`/`pop_result`/`get_entropy`） | `npb/bridge.h` + `npb/src/lib.rs`（cdylib+wasm） | ✅ |
| DOSBox 虚拟喇叭概念演示（C） | `examples/dos_concept/main.c` | ✅ |
| WASM Canvas 呼吸演示（浏览器） | `examples/wasm_canvas/` | ✅ 待浏览器目验 |
| 禅境示波器（官方应用，含思考链/节点显示） | `examples/zen-oscilloscope/`（GitHub Pages 部署中） | 🚧 Phase 3 |

## 仓库结构

```
meta-kernel-core/   ★ 核心代码库（Rust lib，零第三方依赖）
  src/math.rs       模糊饱和运算（a⊕b=min(1,a+b)）与 0-1 归一化
  src/linear.rs     线性引擎（input ⊕ 0.01）
  src/fib.rs        斐波那契引擎（(a+b)×0.5 自参照递归）
  src/expo.rs       指数引擎（input×e^(λΔt)，>0.99 回退 0.5）
  src/hourglass.rs  气泡沙漏：上锥体→瓶颈环形缓冲(破坏性干涉)→下锥体
  src/mirror.rs     镜像池（摩擦源）：回显衰减 + 真空重启
  src/sanitizer.rs  负扰动过滤与安全阀（软钳位/配额休眠/合作奖励/末那识监控）
  src/ontology.rs   0-10 元通用标尺（analyze/decompose/abstract/recompose）
  src/energy.rs     能量层级判定（活力指数 + 四档处置）
  src/positive_source.rs  正源系统（颗粒度/Searcher/Analyzer/Weaver/功德池）
  src/thinking_chain.rs   思考链（存量+变量+补充增量=创新增量；自动降维）
  src/double_chain.rs     双链诊断（问题形成过程 + 解决过程）
  tests/            Phase1 稳定性 + Phase2 安全集成测试
npb/                ★ NPB 桥接器：bridge.h + cdylib/wasm32 实现 + 自检摘要
examples/dos_concept/   C 概念演示（虚拟喇叭）
examples/wasm_canvas/   WASM + JS Canvas 呼吸演示
examples/zen-oscilloscope/  ★ 禅境示波器（官方应用，Phase 3）
examples/tools/          serve.py（一键预览）/ embed_wasm.py（双击直开版）
docs/               数学规范 v1.0 / 0-10 标尺规范 v1.1 / 阶段验收与测试报告
```

## 许可 · License

[Apache-2.0](./LICENSE)

> 注：本项目的核心代码预计长期保持公共资产属性，未来可能捐赠给开放原子开源基金会（OpenAtom）。
