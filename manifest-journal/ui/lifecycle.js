// Manifest Journal UI — 生命周期引擎（JS 复刻，与 npb-appkit lifecycle.rs 同规则）。
// 纯函数模块：CommonJS（node 测试可用）+ 浏览器（window.Lifecycle）。
// 规则源：docs/L4_APPLICATION_FRAMEWORK_DESIGN v1.0 §3。
(function (root, factory) {
  if (typeof module === 'object' && module.exports) { module.exports = factory(); }
  else { root.Lifecycle = factory(); }
})(typeof self !== 'undefined' ? self : this, function () {
  'use strict';

  const RETREAT_WINDOW = 8;   // 99 静默回融窗
  const BAND_DEBOUNCE = 2;    // 跨带防抖事件数

  // 圈层名（与 namer.rs BANDS 同词表）
  const BANDS = ['萌发带','涌动带','凝结带','汇聚带','流淌带','塑形带','结晶带','固化带','极显带'];

  function bandName(state) {
    if (state === 0) return '锚点';
    const b = BANDS[Math.floor(state / 10) - 1] || '?';
    const s = state % 10;
    return state === 99 ? b + '·圆满' : b + '·第' + s + '步';
  }

  function newEngine() { return { state: 0, sinceAwaken: 0, bandPending: 0, rounds: 0 }; }

  // 纯函数 step（对齐 lifecycle.rs）
  function step(state, sinceAwaken, bandPending, ev) {
    if (ev === 'Reset') return [0, 0, 0, false];
    if (ev === 'Awaken') {
      if (state === 0) return [10, 0, 0, false];
      const band = Math.floor(state / 10), stepIn = state % 10;
      if (band >= 9 && stepIn >= 9) return [99, 0, 0, false];
      if (stepIn < 9) return [state + 1, 0, 0, false];
      const pending = bandPending + 1;
      if (pending >= BAND_DEBOUNCE) {
        const nb = band + 1;
        return [nb > 9 ? 99 : nb * 10, 0, 0, true];
      }
      return [state, 0, pending, false];
    }
    // Hold / Settle
    if (state === 0) return [0, 0, 0, false];
    if (state === 99) {
      const sa = sinceAwaken + 1;
      return sa >= RETREAT_WINDOW ? [0, 0, 0, true] : [99, sa, 0, false];
    }
    if (ev === 'Settle') {
      const stepIn = state % 10, band = Math.floor(state / 10);
      if (stepIn > 0) return [state - 1, 0, 0, false];
      const pending = bandPending - 1;
      if (pending <= -BAND_DEBOUNCE) {
        if (band <= 1) return [0, 0, 0, false];
        return [(band - 1) * 10 + 9, 0, 0, true];
      }
      return [state, 0, pending, false];
    }
    return [state, 0, 0, false]; // Hold
  }

  function apply(eng, ev) {
    const [ns, sa, bp, flag] = step(eng.state, eng.sinceAwaken, eng.bandPending, ev);
    if (ns !== eng.state && ns === 0 && eng.state === 99) eng.rounds += 1;
    eng.state = ns; eng.sinceAwaken = sa; eng.bandPending = bp;
    return flag;
  }

  // 事件归一：state_change / instruction JSON → Awaken|Hold|Settle|Reset
  function field(json, key) {
    const m = json.match(new RegExp('"' + key + '"\\s*:\\s*"?([-+.\\d\\w]+)"?'));
    return m ? m[1] : null;
  }
  function normalize(kind, data) {
    if (kind === 'state_change') {
      const f = field(data, 'field');
      if (f === 'low_energy') return field(data, 'to') === '1' ? 'Settle' : 'Awaken';
      const from = parseInt(field(data, 'from') || 'NaN', 10);
      const to = parseInt(field(data, 'to') || 'NaN', 10);
      if (!isNaN(from) && !isNaN(to)) return to < from ? 'Awaken' : to > from ? 'Settle' : 'Hold';
      return 'Hold';
    }
    if (kind === 'instruction') {
      const ty = field(data, 'type') || '';
      if (ty === 'StateChanged') {
        const from = parseInt(field(data, 'from') || 'NaN', 10);
        const to = parseInt(field(data, 'to') || 'NaN', 10);
        if (!isNaN(from) && !isNaN(to)) return to < from ? 'Awaken' : to > from ? 'Settle' : 'Hold';
        return 'Hold';
      }
      if (/LowEnergy|HabitFormed/.test(ty)) return 'Settle';
      if (/CompoundProduced|ResonanceFound|SelfIntensity/.test(ty)) return 'Awaken';
      return 'Hold';
    }
    return 'Hold'; // snapshot / ping
  }

  return { RETREAT_WINDOW, BAND_DEBOUNCE, BANDS, bandName, newEngine, step, apply, normalize, field };
});
