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

WebAssembly.instantiate(bytes, {})
  .then(({ instance }) => {
    const ex = instance.exports;
    const required = [
      "push_seed", "pop_result", "get_entropy",
      "get_thinking_len", "get_thinking_nodes",
      "get_diag_formation", "get_diag_resolution", "get_diag_solved",
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

    const digest = ex.mk_self_test() >>> 0;
    console.log("DIGEST=" + digest);
    console.log("WASM_SMOKE_OK out=" + out.toFixed(4) + " ent=" + ent.toFixed(4) +
      " chain=" + chainLen + "/" + chainNodes);
  })
  .catch((err) => {
    console.error("WASM_LOAD_FAIL:", err);
    process.exit(1);
  });
