/*
 * examples/zen-oscilloscope/zen.js — 存在论 · 波动层版
 *
 * - 0 = 待激发的纯粹存在（能量源）；1 = 第一扰动；物态：能量→气→液→固（黄金阈值）；
 * - 显示：输出/熵/物态/思考链/双链/螺旋度/驻点粒子/触达；
 * - 轨迹图：3D 旋转投影(0.5°/帧) ↔ 2D，斐波那契参考螺旋叠加；
 * - 干涉图案：实时绘制波动干涉驻点图案，色块标注驻点层级；
 * - 进化时间线：步数驱动，每步记录快照，滑杆回放 + 暂停/继续；
 * - 声音：Web Audio 200~1200Hz（点击开）。错误显示于 #err。
 */
(function () {
  "use strict";

  var errBox = document.getElementById("err");
  function showErr(msg) {
    errBox.textContent = String(msg);
    errBox.style.display = "block";
    console.error(msg);
  }

  var canvas = document.getElementById("scope");
  var ctx = canvas.getContext("2d");
  var spark = document.getElementById("spark");
  var sparkCtx = spark.getContext("2d");
  var traj = document.getElementById("traj");
  var trajCtx = traj.getContext("2d");
  var pat = document.getElementById("pattern");
  var patCtx = pat.getContext("2d");
  var SPIRAL = window.SPIRAL;

  var STATE_NAMES = ["能量态", "气态", "液态", "固态"];
  var LAYER_NAMES = ["色", "声", "香/味", "触", "法"];
  var REACH_NAMES = ["L0 自状态", "L1 本地", "L2 网络", "L3 抽象", "L4 演化史"];

  var REQUIRED_EXPORTS = [
    "push_seed", "pop_result", "get_entropy",
    "get_thinking_len", "get_thinking_nodes",
    "get_diag_formation", "get_diag_resolution", "get_diag_solved",
    "get_state", "get_reach_levels", "get_reach_paths",
    "get_interfere_count", "get_interfere_layer", "get_evolution_len",
    "get_self_intensity",
    "get_trace_wind", "get_trace_fire", "get_trace_water", "get_trace_earth"
  ];

  var api = null;
  var outBuf = [];
  var MAX = 96;
  var tick = 0;
  var lastEnt = 0;
  var audioOn = false;
  var audioCtx = null;
  var audioTick = 0;
  var trajMode = "3d";
  var yawDeg = 0;
  var PITCH = 0.5;
  var paused = false;

  var hist = [];           // 进化时间线快照 {t,o,e,s,pc,pt}
  var HIST_CAP = 480;

  function stat(id, text) { document.getElementById(id).textContent = text; }
  function $(id) { return document.getElementById(id); }

  function base64ToBytes(b64) {
    var bin = atob(b64);
    var a = new Uint8Array(bin.length);
    for (var i = 0; i < bin.length; i++) a[i] = bin.charCodeAt(i);
    return a;
  }

  function loadWasm() {
    if (window.NPB_B64) return Promise.resolve(base64ToBytes(window.NPB_B64));
    return fetch("npb.wasm").then(function (r) {
      if (!r.ok) throw new Error("npb.wasm HTTP " + r.status);
      return r.arrayBuffer();
    }).then(function (b) { return new Uint8Array(b); });
  }

  function tint(ent) {
    var hue = 190 - 162 * ent;
    return { fill: "hsl(" + hue + ",68%,60%)", glow: "hsla(" + hue + ",68%,50%," + (0.10 + ent * 0.16) + ")" };
  }

  function beep(freq) {
    if (!audioCtx) return;
    var t = audioCtx.currentTime;
    var osc = audioCtx.createOscillator();
    var gain = audioCtx.createGain();
    osc.type = "sine";
    osc.frequency.value = freq;
    gain.gain.setValueAtTime(0.0001, t);
    gain.gain.exponentialRampToValueAtTime(0.06, t + 0.01);
    gain.gain.exponentialRampToValueAtTime(0.0001, t + 0.09);
    osc.connect(gain).connect(audioCtx.destination);
    osc.start(t);
    osc.stop(t + 0.1);
  }

  /* ---------- 轨迹图 ---------- */

  function refSpiralPoints(meanOut) {
    var pts = [];
    var turns = 1.6;
    var n = 70;
    var GA = SPIRAL ? SPIRAL.GOLDEN_ANGLE : 2.399963229728653;
    for (var i = 0; i <= n; i++) {
      pts.push({ r: Math.max(0.04, meanOut * (0.18 + 0.82 * (i / n))), th: i * GA });
    }
    return pts;
  }

  function project3(p, yaw, pitch, cx, cy, scale) {
    var cY = Math.cos(yaw), sY = Math.sin(yaw);
    var x1 = (p[0] - 0.5) * cY - (p[2] - 0.5) * sY;
    var z1 = (p[0] - 0.5) * sY + (p[2] - 0.5) * cY;
    var cP = Math.cos(pitch), sP = Math.sin(pitch);
    var y1 = (p[1] - 0.5) * cP - z1 * sP;
    var z2 = (p[1] - 0.5) * sP + z1 * cP;
    var persp = 1 / (1 + 0.4 * z2);
    return { x: cx + x1 * scale * persp, y: cy - y1 * scale * persp };
  }

  function drawTraj() {
    var w = traj.width, h = traj.height;
    var cx = w / 2, cy = h / 2;
    var scale = Math.min(w, h) * 0.86;
    trajCtx.clearRect(0, 0, w, h);
    if (outBuf.length < 3) return;
    var c = tint(lastEnt);
    var sum = 0;
    for (var m = 0; m < outBuf.length; m++) sum += outBuf[m];
    var meanOut = sum / outBuf.length;

    if (trajMode === "3d") {
      yawDeg = (yawDeg + 0.5) % 360;
      var yaw = (yawDeg * Math.PI) / 180;
      // 参考螺旋（黄虚线）
      var rp = refSpiralPoints(meanOut);
      trajCtx.strokeStyle = "rgba(239,159,39,0.5)";
      trajCtx.lineWidth = 1;
      trajCtx.setLineDash([3, 3]);
      trajCtx.beginPath();
      rp.forEach(function (p, i) {
        var q = project3([0.5 + p.r * Math.cos(p.th), 0.5 + p.r * Math.sin(p.th), 0.5], yaw, PITCH, cx, cy, scale);
        if (i === 0) trajCtx.moveTo(q.x, q.y); else trajCtx.lineTo(q.x, q.y);
      });
      trajCtx.stroke();
      trajCtx.setLineDash([]);
      // 轨迹
      trajCtx.strokeStyle = c.fill;
      trajCtx.lineWidth = 1.1;
      trajCtx.beginPath();
      for (var i = 0; i <= outBuf.length - 3; i++) {
        var q = project3([outBuf[i], outBuf[i + 1], outBuf[i + 2]], yaw, PITCH, cx, cy, scale);
        if (i === 0) trajCtx.moveTo(q.x, q.y); else trajCtx.lineTo(q.x, q.y);
      }
      trajCtx.stroke();
    } else {
      rp = refSpiralPoints(meanOut);
      trajCtx.strokeStyle = "rgba(239,159,39,0.45)";
      trajCtx.lineWidth = 1;
      trajCtx.setLineDash([3, 3]);
      trajCtx.beginPath();
      rp.forEach(function (p, i) {
        var q = { x: cx + p.r * Math.cos(p.th) * scale * 0.9, y: cy - p.r * Math.sin(p.th) * scale * 0.9 };
        if (i === 0) trajCtx.moveTo(q.x, q.y); else trajCtx.lineTo(q.x, q.y);
      });
      trajCtx.stroke();
      trajCtx.setLineDash([]);
      trajCtx.fillStyle = c.fill;
      for (var j = 0; j < outBuf.length - 1; j++) {
        var x = cx + (outBuf[j + 1] - 0.5) * scale * 0.9;
        var y = cy - (outBuf[j] - 0.5) * scale * 0.9;
        trajCtx.beginPath();
        trajCtx.arc(x, y, 1.3, 0, Math.PI * 2);
        trajCtx.fill();
      }
    }
  }

  function drawWave() {
    var w = spark.width, h = spark.height;
    sparkCtx.clearRect(0, 0, w, h);
    if (outBuf.length < 2) return;
    var c = tint(lastEnt);
    sparkCtx.beginPath();
    for (var i = 0; i < outBuf.length; i++) {
      var x = (i / (MAX - 1)) * w;
      var y = h - 4 - outBuf[i] * (h - 8);
      if (i === 0) sparkCtx.moveTo(x, y); else sparkCtx.lineTo(x, y);
    }
    sparkCtx.strokeStyle = c.fill;
    sparkCtx.lineWidth = 1.5;
    sparkCtx.stroke();
  }

  /* ---------- 干涉图案（3.1/4.3） ---------- */
  function drawPattern(particles, layer) {
    var w = pat.width, h = pat.height;
    patCtx.clearRect(0, 0, w, h);
    if (outBuf.length < 2) return;
    var amp = outBuf[outBuf.length - 1];
    var c = tint(lastEnt);
    for (var x = 0; x < w; x += 2) {
      var k = (x / w) * Math.PI * 4;
      var v = Math.abs(Math.sin(k)) * (0.25 + 0.75 * amp);
      var bh = Math.max(1, v * h);
      patCtx.globalAlpha = 0.25 + v * 0.6;
      patCtx.fillStyle = c.fill;
      patCtx.fillRect(x, h - bh, 2, bh);
    }
    patCtx.globalAlpha = 1;
    // 驻点层级色块
    if (particles > 0) {
      var seg = w / 5;
      var lx = (layer + 0.5) * seg;
      patCtx.fillStyle = "rgba(239,159,39,0.9)";
      patCtx.fillRect(lx - 3, 0, 6, h);
    }
  }

  /* ---------- 主循环 ---------- */

  function liveSnapshot(out, ent, stateCode, pc, pt) {
    hist.push({ t: hist.length, o: out, e: ent, s: stateCode, pc: pc, pt: pt });
    if (hist.length > HIST_CAP) hist.shift();
    var tl = $("tl");
    tl.max = String(hist.length - 1);
    tl.value = String(hist.length - 1);
    $("tlLabel").textContent = "时间线 t=" + hist.length + " · 累计 " + hist.length + " 步";
  }

  function applySnapshot(h) {
    if (!h) return;
    stat("out", h.o.toFixed(3));
    stat("ent", h.e.toFixed(3));
    stat("state", STATE_NAMES[h.s] || "?");
    var cx = canvas.width / 2, cy = canvas.height / 2;
    var radius = 8 + h.o * 120;
    var c = tint(h.e);
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    ctx.beginPath();
    ctx.arc(cx, cy, radius + 16 + h.e * 20, 0, Math.PI * 2);
    ctx.fillStyle = c.glow;
    ctx.fill();
    ctx.beginPath();
    ctx.arc(cx, cy, radius, 0, Math.PI * 2);
    ctx.fillStyle = c.fill;
    ctx.fill();
    ctx.beginPath();
    ctx.arc(cx, cy, 2.5, 0, Math.PI * 2);
    ctx.fillStyle = "#07090d";
    ctx.fill();
    $("tlLabel").textContent = "回放 t=" + h.t + " · " + (STATE_NAMES[h.s] || "") + " · out=" + h.o.toFixed(2) +
      (h.pc > 0 ? " · 粒子 x" + h.pc : "");
  }

  function frame() {
    if (!api) return;
    if (paused) return; // 暂停：仅回放视图，不推进内核

    tick++;
    audioTick++;

    var seed = tick % 24 === 0 ? 1.0 : 0.5 + 0.10 * Math.sin(tick / 9);
    api.push_seed(seed);

    var out = api.pop_result();
    var ent = api.get_entropy();
    var stateCode = api.get_state() >>> 0;
    var clen = api.get_thinking_len();
    var cnodes = api.get_thinking_nodes();
    var f = api.get_diag_formation();
    var r = api.get_diag_resolution();
    var solved = api.get_diag_solved();
    var reach = api.get_reach_levels() >>> 0;
    var paths = api.get_reach_paths() >>> 0;
    var pc = api.get_interfere_count() >>> 0;
    var layer = api.get_interfere_layer() >>> 0;
    var selfI = api.get_self_intensity();
    var twind = api.get_trace_wind() >>> 0;
    var tfire = api.get_trace_fire() >>> 0;
    var twater = api.get_trace_water() >>> 0;
    var tearth = api.get_trace_earth() >>> 0;
    lastEnt = ent;

    var spirality = SPIRAL && outBuf.length >= 12 ? SPIRAL.spirality(outBuf) : 0;

    stat("out", out.toFixed(3));
    stat("ent", ent.toFixed(3));
    stat("state", STATE_NAMES[stateCode] || "未知");
    stat("chainMeta", clen + " / " + cnodes);
    stat("diag", "形成 " + f + " 步 · 解决 " + r + " 步");
    stat("diagState", solved === 1 ? "已收敛" : "收敛中");
    stat("reach", reachText(reach));
    stat("reachPaths", paths + " 路径");
    stat("spiral", spirality.toFixed(3));
    stat("partsCount", pc + " 粒");
    stat("partsLayer", "层:" + (LAYER_NAMES[layer] || "—"));
    stat("selfInt", selfI.toFixed(3));
    stat("traceDist", "风" + twind + " · 火" + tfire + " · 水" + twater + " · 地" + tearth);
    stat("selfFlag", selfI > 0.7 ? "自我识别" : "习气累积");

    if (audioOn && audioCtx && audioTick % 2 === 0) beep(200 + out * 1000);

    outBuf.push(out);
    if (outBuf.length > MAX) outBuf.shift();
    drawWave();
    drawTraj();
    drawPattern(pc, layer);
    liveSnapshot(out, ent, stateCode, pc, layer);

    var cx = canvas.width / 2, cy = canvas.height / 2;
    var radius = 8 + out * 120;
    var c = tint(ent);
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    ctx.beginPath();
    ctx.arc(cx, cy, radius + 18 + ent * 22, 0, Math.PI * 2);
    ctx.fillStyle = c.glow;
    ctx.fill();
    ctx.beginPath();
    ctx.arc(cx, cy, radius, 0, Math.PI * 2);
    ctx.fillStyle = c.fill;
    ctx.fill();
    ctx.beginPath();
    ctx.arc(cx, cy, 2.5, 0, Math.PI * 2);
    ctx.fillStyle = "#07090d";
    ctx.fill();
  }

  function reachText(mask) {
    var parts = [];
    for (var i = 0; i < REACH_NAMES.length; i++) {
      if (mask & (1 << i)) parts.push(REACH_NAMES[i]);
    }
    return parts.length ? parts.join(" + ") : "L0";
  }

  try {
    loadWasm()
      .then(function (bytes) { return WebAssembly.instantiate(bytes, {}); })
      .then(function (res) {
        api = res.instance.exports;
        var missing = REQUIRED_EXPORTS.filter(function (k) { return typeof api[k] !== "function"; });
        if (missing.length) throw new Error("缺少导出函数: " + missing.join(", "));

        api.push_seed(0.01); // 0 = 纯粹存在（待激发），0.01 柔和起搏
        api.push_seed(1.0);  // 1 = 第一扰动（觉醒）
        frame();
        setInterval(frame, 120);
      })
      .catch(function (err) { showErr("初始化失败: " + err); });

    $("seedBtn").addEventListener("click", function () { if (api) api.push_seed(1.0); });

    $("modeBtn").addEventListener("click", function () {
      trajMode = trajMode === "3d" ? "2d" : "3d";
      $("modeBtn").textContent = trajMode === "3d" ? "轨迹模式: 3D 投影" : "轨迹模式: 2D (对比)";
    });

    $("soundBtn").addEventListener("click", function () {
      audioOn = !audioOn;
      if (audioOn) {
        if (!audioCtx) {
          var AC = window.AudioContext || window.webkitAudioContext;
          if (!AC) { audioOn = false; $("soundBtn").textContent = "声音模式: 不支持"; return; }
          audioCtx = new AC();
        }
        if (audioCtx.state === "suspended") audioCtx.resume();
        $("soundBtn").textContent = "声音模式: 开";
        $("soundBtn").classList.add("sound-on");
      } else {
        $("soundBtn").textContent = "声音模式: 关";
        $("soundBtn").classList.remove("sound-on");
      }
    });

    // 进化时间线回放
    $("replayBtn").addEventListener("click", function () {
      paused = !paused;
      $("replayBtn").textContent = paused ? "▶ 继续演化" : "⏸ 暂停回放";
      if (!paused && hist.length) {
        $("tl").value = String(hist.length - 1);
        $("tlLabel").textContent = "时间线 t=" + hist.length;
      }
    });
    $("tl").addEventListener("input", function () {
      var idx = parseInt(this.value, 10);
      if (idx >= 0 && idx < hist.length) {
        paused = true;
        $("replayBtn").textContent = "▶ 继续演化";
        applySnapshot(hist[idx]);
      }
    });
  } catch (err) {
    showErr("脚本错误: " + err);
  }
})();
