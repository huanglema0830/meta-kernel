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
    if (typeof ex.push_seed !== "function" ||
        typeof ex.pop_result !== "function" ||
        typeof ex.get_entropy !== "function" ||
        typeof ex.mk_self_test !== "function") {
      console.error("WASM_EXPORTS_MISSING");
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

    const digest = ex.mk_self_test() >>> 0;
    console.log("DIGEST=" + digest);
    console.log("WASM_SMOKE_OK out=" + out.toFixed(4) + " ent=" + ent.toFixed(4));
  })
  .catch((err) => {
    console.error("WASM_LOAD_FAIL:", err);
    process.exit(1);
  });
