/*
 * examples/zen-oscilloscope/spiral.js — 斐波那契螺旋数学（纯函数，无 DOM 依赖）
 *
 * 供 zen.js（浏览器）与 spiral_test.js（Node CI）共用：
 * - goldenAngle：黄金角 ≈ 2.399963 rad（137.5°）；
 * - refPoint(t, rBase)：理想斐波那契螺旋点（r=随 t 线性外扩，θ=t·黄金角）；
 * - spirality(samples)：当前轨迹与理想螺旋的吻合度（0~1），
 *   由「角步进贴合黄金角」与「半径随时间外扩」两因子合成。
 */
(function (g) {
  "use strict";

  var GOLDEN_RATIO = (1 + Math.sqrt(5)) / 2;
  var GOLDEN_ANGLE = (2 * Math.PI) * (1 - 1 / GOLDEN_RATIO); // ≈2.399963 (137.5°)

  function circularDist(a, b) {
    var d = Math.abs(a - b) % (2 * Math.PI);
    if (d > Math.PI) d = 2 * Math.PI - d;
    return d;
  }

  /* 理想螺旋参考点：r 从 0.25·rBase 线性外扩到 1.0·rBase（t 个采样周期） */
  function refPoint(t, rBase, samples) {
    var r = rBase * (0.25 + 0.75 * ((t % samples) / Math.max(1, samples - 1)));
    return { x: r * Math.cos(t * GOLDEN_ANGLE), y: r * Math.sin(t * GOLDEN_ANGLE) };
  }

  function mean(xs) {
    if (!xs.length) return 0;
    return xs.reduce(function (s, x) { return s + x; }, 0) / xs.length;
  }

  function pearson(xs, ys) {
    var n = Math.min(xs.length, ys.length);
    if (n < 2) return 0;
    var mx = mean(xs), my = mean(ys);
    var num = 0, dx = 0, dy = 0;
    for (var i = 0; i < n; i++) {
      num += (xs[i] - mx) * (ys[i] - my);
      dx += (xs[i] - mx) * (xs[i] - mx);
      dy += (ys[i] - my) * (ys[i] - my);
    }
    if (dx === 0 || dy === 0) return 0;
    return num / Math.sqrt(dx * dy);
  }

  /*
   * spirality(samples: number[]) -> 0..1
   * 把采样序列视为轨迹点 (s[i], s[i+1])：
   *   a = 角步进与黄金角（或其互补）的平均贴合度 ∈[0,1]；
   *   b = 极径随时间的向外扩展趋势（Pearson r 与 0 取大）∈[0,1]；
   *   score = clamp01(0.6a + 0.4b)
   */
  function spirality(samples) {
    if (!Array.isArray(samples) || samples.length < 12) return 0;
    var pts = [];
    for (var i = 0; i < samples.length - 1; i++) {
      pts.push({ x: samples[i], y: samples[i + 1] });
    }
    var cx = mean(pts.map(function (p) { return p.x; }));
    var cy = mean(pts.map(function (p) { return p.y; }));
    var angs = pts.map(function (p) { return Math.atan2(p.y - cy, p.x - cx); });
    var rs = pts.map(function (p) {
      var dx = p.x - cx, dy = p.y - cy;
      return Math.sqrt(dx * dx + dy * dy);
    });
    var comp = 2 * Math.PI - GOLDEN_ANGLE;
    var errSum = 0;
    for (var k = 1; k < angs.length; k++) {
      var step = angs[k] - angs[k - 1];
      errSum += Math.min(circularDist(step, GOLDEN_ANGLE), circularDist(step, comp));
    }
    var eMean = errSum / Math.max(1, angs.length - 1); // ∈[0,π]
    var a = 1 - eMean / Math.PI;
    var corr = pearson(rs.map(function (_, i) { return i; }), rs);
    var b = Math.max(0, corr);
    var score = 0.6 * a + 0.4 * b;
    return Math.max(0, Math.min(1, score));
  }

  var api = { GOLDEN_RATIO: GOLDEN_RATIO, GOLDEN_ANGLE: GOLDEN_ANGLE, refPoint: refPoint, spirality: spirality };
  if (typeof module !== "undefined" && module.exports) module.exports = api;
  if (g) g.SPIRAL = api;
})(typeof window !== "undefined" ? window : null);
