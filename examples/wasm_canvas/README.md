# wasm_canvas — 浏览器示例（同一内核，WASM 直驱）

## 构建

```bash
# 仓库根目录
rustup target add wasm32-unknown-unknown
cargo build -p npb --release --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/npb.wasm examples/wasm_canvas/npb.wasm
```

## 运行

```bash
cd examples/wasm_canvas
python3 -m http.server 8000
# 浏览器打开 http://localhost:8000
```

页面说明：圆点半径 = `pop_result()` 内核输出；晕圈/色调 = `get_entropy()`；
按钮 = 手动注入扰动（1.0）。所有数值均来自 WASM 内的同一套 `meta-kernel-core`。

## 无头校验（CI 用）

```bash
node self_test.js npb.wasm
# 输出 DIGEST=<u32>，与原生侧一致即通过跨平台一致性
```
