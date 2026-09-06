/*
 * examples/wasm_canvas/self_test.js — 无头跨平台一致性校验
 *
 * 用法：node self_test.js [npb.wasm 路径]
 * 默认读取本目录 npb.wasm（构建后请复制过去），也可传入 CI 构建产物路径。
 * 输出 DIGEST=<u32>，与原生侧 `cargo run -p npb --example self_test`
 * 的 DIGEST 一致即证明：同一套内核代码原生/WASM 行为一致。
 */
const fs = require("fs");
const path = require("path");

const wasmPath = process.argv[2] || path.join(__dirname, "npb.wasm");
const bytes = fs.readFileSync(wasmPath);

// 从 WASM 线性内存读取 C 字符串（用于 pop_instruction_json 的返回指针）。
function readCStr(mem, ptr) {
  if (!mem || !ptr) return "";
  const u8 = new Uint8Array(mem.buffer);
  let s = "";
  let i = ptr;
  while (i < u8.length && u8[i] !== 0) {
    s += String.fromCharCode(u8[i]);
    i++;
  }
  return s;
}

WebAssembly.instantiate(bytes, {})
  .then(({ instance }) => {
    const ex = instance.exports;
    const required = [
      "push_seed", "pop_result", "get_entropy",
      "get_thinking_len", "get_thinking_nodes",
      "get_diag_formation", "get_diag_resolution", "get_diag_solved",
      "get_state", "get_state_budget", "get_reach_levels", "get_reach_paths",
      "get_interfere_count", "get_interfere_layer", "get_evolution_len",
      "get_self_intensity",
      "get_trace_wind", "get_trace_fire", "get_trace_water", "get_trace_earth",
      "get_energy_absorbed", "get_energy_spent", "get_energy_ratio", "get_energy_stored", "get_product_energy",
      "get_anchor_distance", "get_anchor_band", "get_instruction_count",
      "pop_instruction_json", "free_instruction_json",
      "get_mirror_dominant", "get_mirror_in_phase",
      "get_gate_pass_count", "get_gate_recycle_count", "get_gate_reject_count",
      "mk_self_test"
    ];
    const missing = required.filter((k) => typeof ex[k] !== "function");
    if (missing.length) {
      console.error("WASM_EXPORTS_MISSING: " + missing.join(","));
      process.exit(1);
    }

    // 三接口冒烟
    ex.push_seed(1.0);
    const out = ex.pop_result();
    const ent = ex.get_entropy();
    if (!(out >= 0.0 && out <= 1.0) || !(ent >= 0.0 && ent <= 1.0)) {
      console.error("WASM_SMOKE_RANGE_FAIL out=" + out + " ent=" + ent);
      process.exit(1);
    }

    // 思考链自动递增校验：5 次注入后 length 应增长且非 0
    for (let i = 0; i < 5; i++) ex.push_seed(0.01 + i * 0.2);
    const chainLen = ex.get_thinking_len() >>> 0;
    const chainNodes = ex.get_thinking_nodes() >>> 0;
    if (chainLen < 1 || chainNodes < 1 || chainNodes > chainLen) {
      console.error("CHAIN_STATS_FAIL len=" + chainLen + " nodes=" + chainNodes);
      process.exit(1);
    }

    // Phase 4：物态码 0..3；触达 L0 必在，路径数非负
    const state = ex.get_state() >>> 0;
    const reach = ex.get_reach_levels() >>> 0;
    const paths = ex.get_reach_paths() >>> 0;
    if (state > 3 || (reach & 1) !== 1) {
      console.error("STATE_REACH_FAIL state=" + state + " reach=" + reach);
      process.exit(1);
    }
    if (typeof paths !== "number" || paths < 0) {
      console.error("REACH_PATHS_FAIL paths=" + paths);
      process.exit(1);
    }

    // 波动层导出：干涉计数/层级/进化长度类型与范围
    const interfere = ex.get_interfere_count() >>> 0;
    const layer = ex.get_interfere_layer() >>> 0;
    const evoLen = ex.get_evolution_len() >>> 0;
    if (layer > 4 || typeof interfere !== "number" || typeof evoLen !== "number") {
      console.error("WAVE_LAYER_FAIL interfere=" + interfere + " layer=" + layer + " evo=" + evoLen);
      process.exit(1);
    }

    // 痕迹层导出：自我感 0..1；痕迹计数非负数字
    const selfI = ex.get_self_intensity();
    const tw = ex.get_trace_wind() >>> 0;
    const tfire = ex.get_trace_fire() >>> 0;
    const twater = ex.get_trace_water() >>> 0;
    const tearth = ex.get_trace_earth() >>> 0;
    if (!(selfI >= 0.0 && selfI <= 1.0) ||
        typeof tw !== "number" || typeof tfire !== "number" ||
        typeof twater !== "number" || typeof tearth !== "number") {
      console.error("TRACE_EXPORT_FAIL self=" + selfI + " w/f/wa/e=" + tw + "/" + tfire + "/" + twater + "/" + tearth);
      process.exit(1);
    }

    // 能量流导出：入/出/比/产物均为有限数值，比值>0
    const ea = ex.get_energy_absorbed();
    const es = ex.get_energy_spent();
    const er = ex.get_energy_ratio();
    const est = ex.get_energy_stored();
    const pe = ex.get_product_energy();
    const sb = ex.get_state_budget() >>> 0;
    if (![ea, es, er, pe].every(Number.isFinite) || er <= 0) {
      console.error("ENERGY_EXPORT_FAIL ea/es/er/pe=" + ea + "/" + es + "/" + er + "/" + pe);
      process.exit(1);
    }
    if (!(est >= 0.0 && est <= 1.0)) {
      console.error("ENERGY_STORED_FAIL est=" + est);
      process.exit(1);
    }
    if (sb > 3) {
      console.error("STATE_BUDGET_FAIL sb=" + sb);
      process.exit(1);
    }

    // 心海全景：anchor_distance ∈ [0,1]，分带索引 ∈ [0,3]
    for (let i = 0; i < 60; i++) ex.push_seed(((i % 11) / 11));
    const ad = ex.get_anchor_distance();
    const ab = ex.get_anchor_band() >>> 0;
    if (!(ad >= 0.0 && ad <= 1.0)) {
      console.error("ANCHOR_DIST_FAIL ad=" + ad);
      process.exit(1);
    }
    if (ab > 3) {
      console.error("ANCHOR_BAND_FAIL ab=" + ab);
      process.exit(1);
    }

    // 思流照亮：指令队列非空时，pop 出的 JSON 可解析且含 type 字段
    const instrCount = ex.get_instruction_count() >>> 0;
    if (typeof instrCount !== "number" || instrCount < 0) {
      console.error("INSTR_COUNT_FAIL n=" + instrCount);
      process.exit(1);
    }
    let jsonSample = "";
    if (instrCount > 0) {
      const ptr = ex.pop_instruction_json();
      if (typeof ptr !== "number") {
        console.error("POP_INSTR_PTR_FAIL");
        process.exit(1);
      }
      jsonSample = readCStr(ex.memory, ptr);
      if (ptr !== 0) ex.free_instruction_json(ptr); // 必须释放，避免泄漏
      if (jsonSample.length > 0) {
        try {
          const obj = JSON.parse(jsonSample);
          if (!obj.type) throw new Error("缺 type 字段");
        } catch (e) {
          console.error("INSTR_JSON_FAIL: " + jsonSample);
          process.exit(1);
        }
      }
    }

    // 摩尼宝珠：镜面主导相位 ∈ [0,2π) 有限；闸门三类计数之和 == 已推入次数(66)，拒绝数=0
    const md = ex.get_mirror_dominant();
    const mip = ex.get_mirror_in_phase() >>> 0;
    const gp = ex.get_gate_pass_count() >>> 0;
    const gr = ex.get_gate_recycle_count() >>> 0;
    const gd = ex.get_gate_reject_count() >>> 0;
    if (!Number.isFinite(md) || md < 0 || md > 2 * Math.PI) {
      console.error("MIRROR_DOMINANT_FAIL md=" + md);
      process.exit(1);
    }
    if (typeof mip !== "number" || gp + gr + gd !== 66) {
      console.error("GATE_SUM_FAIL pass/recycle/reject=" + gp + "/" + gr + "/" + gd + " (期待 66)");
      process.exit(1);
    }
    if (gd !== 0) {
      console.error("GATE_NEGATIVE_FAIL gd=" + gd + "（全部非负推入，拒绝应为 0）");
      process.exit(1);
    }

    const digest = ex.mk_self_test() >>> 0;
    console.log("DIGEST=" + digest);
    console.log("WASM_SMOKE_OK out=" + out.toFixed(4) + " ent=" + ent.toFixed(4) +
      " chain=" + chainLen + "/" + chainNodes +
      " state=" + state + " reach=" + reach + " paths=" + paths +
      " interfere=" + interfere + "/" + layer + " evo=" + evoLen +
      " self=" + selfI.toFixed(3) + " trace=" + tw + "/" + tfire + "/" + twater + "/" + tearth +
      " energy=" + ea.toFixed(3) + "/" + es.toFixed(3) + " r=" + er.toFixed(2) + " store=" + est.toFixed(3) + " sb=" + sb + " prod=" + pe.toFixed(3) +
      " anchor=" + ad.toFixed(3) + " band=" + ab + " instr=" + instrCount +
      " mirror=" + md.toFixed(3) + " inPhase=" + mip + " gate=" + gp + "/" + gr + "/" + gd);
  })
  .catch((err) => {
    console.error("WASM_LOAD_FAIL:", err);
    process.exit(1);
  });
