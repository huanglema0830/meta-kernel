/*
 * examples/zen-oscilloscope/zen.js — Phase 4 轨迹微调版
 *
 * - 圆：半径 = pop_result()；色温随熵联动（低熵冷青 ↔ 高熵暖金）；
 * - 显示：输出 / 熵 / 物态 / 思考链·节点 / 双链 / 触达 / 螺旋度(spirality)；
 * - 轨迹图升级：
 *   ① 三维投影：连续三次输出为 (x,y,z)，动态旋转投影（每帧 0.5°），立体螺旋/分形；
 *   ② 斐波那契螺旋参考线：半径=当前输出均值，角度=时间×黄金角（137.5°）叠加对比；
 *   ③ 螺旋度评分 spirality ∈[0,1]（角步进贴合黄金角 × 半径外扩趋势）；
 *   ④ 3D / 2D 可切换（2D 保留对比）。
 * - 声音模式：Web Audio 输出 200~1200 Hz，需点击开启（浏览器策略）；
 * - 加载即呼吸：0.01 初始扰动 + 首帧渲染；错误显示于 #err。
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
  var SPIRAL = window.SPIRAL;

  var STATE_NAMES = ["固态", "液态", "气态", "等离子态"];
  var REACH_NAMES = ["L0 自状态", "L1 本地", "L2 网络", "L3 抽象", "L4 演化史"];

  var REQUIRED_EXPORTS = [
    "push_seed", "pop_result", "get_entropy",
    "get_thinking_len", "get_thinking_nodes",
    "get_diag_formation", "get_diag_resolution", "get_diag_solved",
    "get_state", "get_reach_levels", "get_reach_paths"
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

  function stat(id, text) { document.getElementById(id).textContent = text; }

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

  /* ---------- 轨迹图（2D / 3D） ---------- */

  function polarXY(r, th, cx, cy, scale) {
    return { x: cx + r * scale * Math.cos(th), y: cy - r * scale * Math.sin(th) };
  }

  /* 斐波那契参考螺旋：半径围绕 meanOut 外扩，角度 = t×黄金角（1.6 圈） */
  function refSpiralPoints(meanOut) {
    var pts = [];
    var turns = 1.6;
    var n = 70;
    var GA = SPIRAL ? SPIRAL.GOLDEN_ANGLE : 2.399963229728653;
    for (var i = 0; i <= n; i++) {
      var th = i * GA;
      var grow = 0.18 + 0.82 * (i / n);
      pts.push({ r: Math.max(0.04, meanOut * grow), th: th });
    }
    return pts;
  }

  function drawRef2d(cx, cy, scale, meanOut) {
    var pts = refSpiralPoints(meanOut);
    trajCtx.strokeStyle = "rgba(239,159,39,0.45)";
    trajCtx.lineWidth = 1;
    trajCtx.setLineDash([3, 3]);
    trajCtx.beginPath();
    pts.forEach(function (p, i) {
      var q = polarXY(p.r, p.th, cx, cy, scale);
      if (i === 0) trajCtx.moveTo(q.x, q.y); else trajCtx.lineTo(q.x, q.y);
    });
    trajCtx.stroke();
    trajCtx.setLineDash([]);
  }

  /* 正交投影：p=[x,y,z]∈[0,1]³，绕 Y 旋转 yaw，绕 X 倾斜 pitch */
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

  function drawRef3d(cx, cy, scale, meanOut, yaw) {
    var pts = refSpiralPoints(meanOut);
    trajCtx.strokeStyle = "rgba(239,159,39,0.5)";
    trajCtx.lineWidth = 1;
    trajCtx.setLineDash([3, 3]);
    trajCtx.beginPath();
    pts.forEach(function (p, i) {
      var x = 0.5 + p.r * Math.cos(p.th);
      var y = 0.5 + p.r * Math.sin(p.th);
      var q = project3([x, y, 0.5], yaw, PITCH, cx, cy, scale);
      if (i === 0) trajCtx.moveTo(q.x, q.y); else trajCtx.lineTo(q.x, q.y);
    });
    trajCtx.stroke();
    trajCtx.setLineDash([]);
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
      yawDeg = (yawDeg + 0.5) % 360; // 每帧旋转 0.5°
      var yaw = (yawDeg * Math.PI) / 180;
      drawRef3d(cx, cy, scale, meanOut, yaw);
      trajCtx.strokeStyle = c.fill;
      trajCtx.lineWidth = 1.1;
      trajCtx.beginPath();
      for (var i = 0; i <= outBuf.length - 3; i++) {
        var q = project3([outBuf[i], outBuf[i + 1], outBuf[i + 2]], yaw, PITCH, cx, cy, scale);
        if (i === 0) trajCtx.moveTo(q.x, q.y); else trajCtx.lineTo(q.x, q.y);
      }
      trajCtx.stroke();
    } else {
      drawRef2d(cx, cy, scale * 0.9, meanOut);
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

  /* ---------- 主循环 ---------- */

  function frame() {
    if (!api) return;
    tick++;
    audioTick++;

    var seed = tick % 24 === 0 ? 1.0 : 0.5 + 0.10 * Math.sin(tick / 9);
    if (tick % 61 === 0) { api.push_seed(0.9); api.push_seed(0.85); }
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

    if (audioOn && audioCtx && audioTick % 2 === 0) beep(200 + out * 1000);

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

    outBuf.push(out);
    if (outBuf.length > MAX) outBuf.shift();
    drawWave();
    drawTraj();
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

        api.push_seed(0.01);
        api.push_seed(1.0);
        frame();
        setInterval(frame, 120);
      })
      .catch(function (err) { showErr("初始化失败: " + err); });

    document.getElementById("seedBtn").addEventListener("click", function () {
      if (api) api.push_seed(1.0);
    });

    document.getElementById("modeBtn").addEventListener("click", function () {
      trajMode = trajMode === "3d" ? "2d" : "3d";
      document.getElementById("modeBtn").textContent =
        trajMode === "3d" ? "轨迹模式: 3D 投影" : "轨迹模式: 2D (对比)";
    });

    document.getElementById("soundBtn").addEventListener("click", function () {
      audioOn = !audioOn;
      var b = document.getElementById("soundBtn");
      if (audioOn) {
        if (!audioCtx) {
          var AC = window.AudioContext || window.webkitAudioContext;
          if (!AC) { audioOn = false; b.textContent = "声音模式: 不支持"; return; }
          audioCtx = new AC();
        }
        if (audioCtx.state === "suspended") audioCtx.resume();
        b.textContent = "声音模式: 开";
        b.classList.add("sound-on");
      } else {
        b.textContent = "声音模式: 关";
        b.classList.remove("sound-on");
      }
    });
  } catch (err) {
    showErr("脚本错误: " + err);
  }
})();
