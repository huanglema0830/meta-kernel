# L3 应用层接入口 · 设计方案（API_GATEWAY_DESIGN）v1.0

> 设计状态：**定稿（v1.0 · 2026-09-07，发起人审阅确认）** ｜ 阶段：L3 设计（只设计，实现待流水线）
> 前置：A4/A5 复核收尾（commit cbf1b1d，本地）｜ 内核 HEAD 基线：277c749（A·观察 v1）
> 相关文档：MATH_SPEC v1.1、ONTOLOGY_SPEC、PHASE1_ASSUMPTIONS_REVIEW.md

---

## 1. 背景与目标

内核（meta-kernel-core + npb C FFI）已具备完整能力面：状态机（能量池/物态/自我感/痕迹/习气）、摩尼宝珠闭环（镜面→闸门→回归）、持久化（刷新不归零）、观测与回放（A·观察 v1）。当前仅浏览器示波器经 wasm 直连使用。

**L3 目标**：为外部程序提供标准化的**内核调用接口**，使任何应用可以：
1. **注入扰动**（向内核 push 种子）；
2. **订阅内核状态变化**（物态/储备/自我感/心海全景等翻转事件）；
3. **接收内核发出的指令**（思流照亮 JSON 指令流）。

**本轮边界**：只交付本设计文档；实现（网关代码）按 A 类新模块走完整流水线（专家审核 → 实现 → CI → 验收），另起实现轮。L4（应用框架）、L5/L6（应用层）**不在本阶段展开**，但本设计须满足 §7 通用性要求以支撑其扩展。

## 2. 设计铁律（不可违背）

| # | 铁律 | 落点 |
|---|---|---|
| R1 | **界面/网关 = 内核真实状态的投影** | 网关所有对外读数**全部来自 FFI 直读**（42 导出）；禁止网关维护"第二份业务状态"、禁止缓存漂移 |
| R2 | **内核零改动、零依赖** | 网关为纯新增壳（新 crate/进程），不触碰 meta-kernel-core/npb 源码；内核 tick 语义不变 |
| R3 | **不空转驱动** | 内核 tick 由外部 `push` 驱动（现状）；网关不伪造空输入推进演化 |
| R4 | **指令通道只转发不解释** | 思流照亮指令 JSON 由内核产出（`pop_instruction_json`），网关原样投递 |
| R5 | **接口契约稳定、语义开放** | 快照/事件 schema 版本化；事件类型集开放，供 L4-L6 扩展 |

## 3. 分层与接口形态路线

```
L4-L6（应用框架/应用层）────────────── 本阶段不展开；预留扩展点（§7）
        │  经 L3 标准接口
L3 网关层（本设计）── HTTP+SSE（一期）→ WebSocket（二期）→ 必要时 IPC
        │  全部读数 = npb C ABI（42 导出，FFI 直读）
L1-L2 内核层（已完成）── meta-kernel-core（纯数学）/ npb 桥（C ABI / wasm）
```

| 决策点 | 结论 | 依据 |
|---|---|---|
| 一期传输 | **HTTP（请求-响应）+ SSE（事件流）** | 调试友好、生态广；事件流由 SSE 单向推送满足"订阅状态+收指令" |
| 二期传输 | **WebSocket（双向）** | 高频双向交互扩展；协议与一期共享语义层，仅换传输 |
| 承载模式 | **A：宿主进程嵌入原生库起步**（Rust/Python 宿主直接链接 cdylib） | 零序列化损耗、验证协议最快；B（独立网关+子进程）与 C（wasm 同构）留作部署形态候选 |
| 驱动语义 | 内核 tick = 外部 push 驱动；网关**不空转** | 与现状一致，保演化语义纯净 |

## 4. 协议规范 v0（基线端点）

| 端点 | 方法 | 语义 | 备注 |
|---|---|---|---|
| `/v1/push` | POST | 注入扰动 `{seed: f32}` | 负值由内核拒绝 → 响应 `{accepted:false, reason:"gate_rejected"}`；`tag` 为预留扩展位（§4.3），可缺省 |
| `/v1/state` | GET | 当前内核快照 | 单对象；`?since=<t>` 返回增量事件集（未来扩展） |
| `/v1/events` | GET | SSE 订阅流 | 事件：`state_change` / `instruction` / `snapshot`（节拍心跳）/ `ping` |
| `/v1/persist/snapshot` | POST | 取内核快照 JSON（直通 `persist_snapshot_json`） | 供外部备份 |
| `/v1/persist/restore` | POST | 恢复快照（直通 `persist_load_buf_ptr` + `persist_apply`） | 失败返回 `{ok:false}`，内核继续 0 锚点 |
| `/v1/health` | GET | `{digest:<mk_self_test()>, ok:true}` | 跨平台一致性可观测 |

> 认证/限流：一期留空位（受信本机/内网），接入点做成可插拔，不阻塞协议验证。

### 4.1 快照 schema（v1，字段全部 FFI 直读）

```json
{
  "schema": 1,
  "t": 1024,
  "state": {"flow": 0, "budget": 1},
  "energy": {"stored": 0.42, "ratio": 1.35, "absorbed": 3.1, "spent": 2.3},
  "self": 0.28,
  "anchor": {"distance": 0.31, "band": 1},
  "mirror": {"dominant": 2.71, "in_phase": 3},
  "gate": {"pass": 9996, "recycled": 4, "rejected": 0},
  "entropy": 0.61
}
```

字段 ↔ 导出映射（全部 `get_*` 直读）：flow=`get_state`、budget=`get_state_budget`、energy 四值=`get_energy_*`、self=`get_self_intensity`、anchor=`get_anchor_distance/get_anchor_band`、mirror=`get_mirror_dominant/get_mirror_in_phase`、gate 三值=`get_gate_*`、entropy=`get_entropy`。（A·观察台账尾值 `get_energy_trace_len`/`energy_last` 视需要并入 v1.1。）

### 4.2 SSE 事件类型（v1）

- `event: state_change`：字段值翻转（边沿）才推——`{t, field, from, to}`（field ∈ flow/budget/band/self 档位/低能量预警等）；
- `event: instruction`：思流照亮指令原样 JSON（网关周期 drain `pop_instruction_json`→`free_instruction_json`）；
- `event: snapshot`：网关节拍（默认 ~10 Hz，可配）广播全量快照，供新订阅者秒同步；
- `event: ping`：保活。

推送节拍与缓冲：默认 10 Hz；缓冲上限防慢消费者压垮网关（丢旧保新，策略一期最简单化）。

### 4.3 因果标注 tag（扩展位，暂不实现）

请求与事件 schema 均**预留 `tag` 字段位**（`"tag": null`），用于未来多应用隔离/审计/因果回放；一期不强制、不校验、不实现。仅在 §7 通用性中承诺字段位不被占用。

## 5. 订阅模型（守 R1：真实状态投影）

```
外部 push ──► [内核演进 1 tick] ──► FFI 直读快照 ──► 边沿 diff
                                               ├─ 有翻转 ──► SSE state_change
                                               └─ 有指令 ──► drain ──► SSE instruction
网关定时(10Hz)全量快照 ──────────────────────► SSE snapshot（秒同步）
```

- 网关只做"读 + 比 + 转"：**不缓存业务态、不派生状态、不解释指令**；
- `state_change` 边沿定义与内核自身事件口径一致（物态切换、自我感 Δ≥0.1、低能量边沿等，见内核 executor 阈值），避免网关另造事件语义。

## 6. 承载模式 A 起步要点（实现轮用）

- 宿主进程（Rust 优先；Python ctypes 亦可行）链接 npb cdylib（原生 42 导出）；
- 网关进程单内核实例、串行 push（内核非线程安全——wasm 单线程同构语义，native 亦按单线程持有）；
- 快照与事件 JSON 由网关侧手写序列化（零第三方依赖惯例），或宿主语言自带 JSON 库（网关允许 std 生态，不违反内核零依赖）。

## 7. 通用性要求（支撑 L4-L6 扩展，本阶段不展开 L4-L6）

1. **传输无关**：语义层与传输解耦——协议文档化的 JSON schema 即契约；二期 WebSocket / 未来 IPC 只换通道，不改语义；
2. **事件类型开放**：`state_change`/`instruction` 为已定义最小集；schema 版本化（`schema` 字段 + 兼容性规则：只增不改、旧版本可读），L4 应用框架可挂新事件类型；
3. **tag 扩展位保留**（§4.3），为多应用接入预留隔离/审计钩子；
4. **快照字段来自单一真相源**（内核 FFI），L4-L6 任何应用所见即内核所是；
5. **接入面最小**：外部应用只需 `push`（写入）+ 订阅（读取）两种动作即可完整驱动/观察内核——应用框架只需实现事件分发与状态投影，无需理解内核内部 42 函数。

## 8. 落地路线（定稿后，另起实现轮执行）

A 类新模块 → 完整流水线：**专家审核本设计 → 实现（网关 crate + HTTP/SSE）→ 测试入 CI → 验收（目验清单 + 协议联调）**。
实现轮先决事项：①确认宿主语言（Rust 建议）；②确认节拍/缓冲参数；③确认识别与限流策略；④确认是否与 A·观察 v1 目验合并验收。

## 9. 风险与边界

- **单实例单连接语义**：内核为单实例状态机，多客户端共享同一演化（一期明确"一内核多订阅者、单一写者"）；
- **push 无并发保护**：多写者需网关层串行化（单写者即可，二期再议）；
- 本设计**不新增任何内核 FFI**（42 导出够用）；若后续需 `energy_last` 快照值等，走 FFI 扩展的 B 类改动另议；
- L4-L6 明确不展开，本文件不预写其规划。

## 10. 版本记录

- v1.0（2026-09-07）：发起人审阅确认（方向 HTTP+SSE→WS、模式 A、tag 扩展位、A 类流水线），定稿成档。
