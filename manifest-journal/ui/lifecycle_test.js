// Manifest Journal UI — 生命周期 JS 引擎单测（node 无浏览器运行；CI 与 spiral_test 同惯例）
const L = require('./lifecycle.js');
const assert = require('assert');

// 1) 0 点亮 → 10
assert.strictEqual(L.step(0, 0, 0, 'Awaken')[0], 10);
// 2) 同带逐个推进
let s = 10;
for (const expect of [11, 12, 13, 14, 15, 16, 17, 18, 19]) {
  s = L.step(s, 0, 0, 'Awaken')[0];
  assert.strictEqual(s, expect);
}
// 3) 19→20 跨带需两次
const [s1, , p1] = L.step(19, 0, 0, 'Awaken');
assert.strictEqual(s1, 19); assert.strictEqual(p1, 1);
assert.strictEqual(L.step(s1, 0, p1, 'Awaken')[0], 20);
// 4) 99 静默窗 N=8 回融
let st = 99, sa = 0, retreated = false;
for (let i = 1; i <= L.RETREAT_WINDOW + 2; i++) {
  const [ns, nsa] = L.step(st, sa, 0, 'Hold');
  if (ns === 0) { retreated = true; assert.strictEqual(i, L.RETREAT_WINDOW); break; }
  st = ns; sa = nsa;
}
assert.ok(retreated, '回融窗');
// 5) 轮次记账
const eng = L.newEngine();
L.apply(eng, 'Awaken');
let guard = 0;
while (eng.state !== 99 && guard++ < 300) L.apply(eng, 'Awaken');
assert.strictEqual(eng.state, 99);
guard = 0;
while (eng.state !== 0 && guard++ < 30) L.apply(eng, 'Hold');
assert.strictEqual(eng.state, 0);
assert.strictEqual(eng.rounds, 1);
// 6) 归一：state_change 方向 / instruction / low_energy
assert.strictEqual(L.normalize('state_change', '{"field":"budget","from":2,"to":0}'), 'Awaken');
assert.strictEqual(L.normalize('state_change', '{"field":"flow","from":0,"to":3}'), 'Settle');
assert.strictEqual(L.normalize('state_change', '{"field":"low_energy","from":0,"to":1}'), 'Settle');
assert.strictEqual(L.normalize('instruction', '{"type":"ResonanceFound","twin_fingerprint":7}'), 'Awaken');
assert.strictEqual(L.normalize('instruction', '{"type":"LowEnergy"}'), 'Settle');
assert.strictEqual(L.normalize('snapshot', '{}'), 'Hold');
// 7) 圈层名
assert.strictEqual(L.bandName(0), '锚点');
assert.strictEqual(L.bandName(55), '流淌带·第5步');
assert.strictEqual(L.bandName(99), '极显带·圆满');
console.log('LIFECYCLE_JS_OK 7 groups');
