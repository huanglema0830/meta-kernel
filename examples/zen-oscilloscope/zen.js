/*
 * examples/zen-oscilloscope/zen.js — Phase 4
 *
 * 禅境示波器逻辑：WASM 内核直驱。
 * - 圆：半径 = pop_result()；色温随熵联动（低熵冷青 ↔ 高熵暖金）；
 * - 显示：内核输出 / 熵 / 当前物态(get_state) / 思考链·推演节点 /
 *   双链诊断 / 正源触达范围与路径(get_reach_levels / get_reach_paths)；
 * - 轨迹图（1.4）：XY 散点（x=当前输出，y=上一输出），观察螺旋/分形/吸引子；
 * - 声音模式（1.4）：Web Audio 播放输出对应音调（200~1200 Hz），需点击开；
 * - 波形：时间序列；
 * - 加载即呼吸：0.01 初始扰动 + 立即渲染首帧；
 * - 错误显示在 #err 横幅。
 * wasm：内嵌 base64（双击）或 fetch（服务器）。
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
  var trajPts = []; // [prev, curr]
  var MAX = 96;
  var TRAJ_MAX = 260;
  var tick = 0;
  var prevOut = 0;
  var lastEnt = 0;
  var audioOn = false;
  var audioCtx = null;
  var audioTick = 0;

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

  function drawTraj() {
    var w = traj.width, h = traj.height;
    trajCtx.clearRect(0, 0, w, h);
    if (trajPts.length < 4) return;
    var c = tint(lastEnt);
    trajCtx.fillStyle = c.fill;
    for (var i = 0; i < trajPts.length; i++) {
      var x = 6 + trajPts[i][0] * (w - 12);
      var y = h - 6 - trajPts[i][1] * (h - 12);
      trajCtx.beginPath();
      trajCtx.arc(x, y, 1.4, 0, Math.PI * 2);
      trajCtx.fill();
    }
  }

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

    stat("out", out.toFixed(3));
    stat("ent", ent.toFixed(3));
    stat("state", STATE_NAMES[stateCode] || "未知");
    stat("chainMeta", clen + " / " + cnodes);
    stat("diag", "形成 " + f + " 步 · 解决 " + r + " 步");
    stat("diagState", solved === 1 ? "已收敛" : "收敛中");
    stat("reach", reachText(reach));
    stat("reachPaths", paths + " 路径");

    // 声音：输出 0-1 → 200-1200 Hz
    if (audioOn && audioCtx && audioTick % 2 === 0) beep(200 + out * 1000);

    // 轨迹点
    trajPts.push([prevOut, out]);
    if (trajPts.length > TRAJ_MAX) trajPts.shift();
    prevOut = out;

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
