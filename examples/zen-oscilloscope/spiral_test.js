/*
 * spiral_test.js — spirality 纯函数 CI 校验（无 wasm、无 DOM）
 * 断言：理想斐波那契螺旋 → 高螺旋度；随机游走 → 低螺旋度。
 * 用法: node spiral_test.js
 */
const S = require("./spiral.js");

function goldenSpiralSamples(n, r0) {
  const out = [];
  for (let i = 0; i < n; i++) {
    const r = r0 * (0.3 + 0.7 * (i / (n - 1))); // 半径线性外扩 → (x_t, x_{t+1}) 成外旋黄金螺旋
    out.push(0.5 + 0.4 * r * Math.cos(i * S.GOLDEN_ANGLE));
  }
  return out;
}

/* 确定性伪随机游走（LCG） */
function randomWalk(n, seed) {
  let x = seed >>> 0;
  const out = [];
  let v = 0.5;
  for (let i = 0; i < n; i++) {
    x = (Math.imul(x, 1664525) + 1013904223) >>> 0;
    v += ((x / 0xffffffff) - 0.5) * 0.18;
    v = Math.max(0.05, Math.min(0.95, v));
    out.push(v);
  }
  return out;
}

const high = S.spirality(goldenSpiralSamples(140, 0.5));
const low = S.spirality(randomWalk(120, 0xC0FFEE));
const flat = S.spirality(Array(120).fill(0.5));

let fail = false;
console.log("spirality(golden spiral) =", high.toFixed(4));
console.log("spirality(random walk)  =", low.toFixed(4));
console.log("spirality(flat)         =", flat.toFixed(4));
if (!(high > 0.7)) { console.error("FAIL: golden spiral should score high"); fail = true; }
if (!(low < 0.6)) { console.error("FAIL: random walk should score low, got " + low); fail = true; }
if (!(flat < 0.4)) { console.error("FAIL: flat series should be low, got " + flat); fail = true; }
if (!(S.GOLDEN_ANGLE > 2.3 && S.GOLDEN_ANGLE < 2.5)) { console.error("FAIL: golden angle range"); fail = true; }
if (fail) process.exit(1);
console.log("SPIRAL_TEST_OK");
