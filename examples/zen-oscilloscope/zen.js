/*
 * examples/zen-oscilloscope/zen.js
 *
 * 禅境示波器逻辑：WASM 内核直驱。
 * - 圆：半径 = pop_result()（内核输出）；色温随熵联动（低熵冷青 ↔ 高熵暖金）；
 * - 数据区：思考链长度（get_thinking_len）、推演节点数（get_thinking_nodes）、
 *   双链诊断（get_diag_formation / get_diag_resolution / get_diag_solved）；
 * - 底部波形：输出时间序列，颜色随熵值联动（与主圆同色调）；
 * - 加载即呼吸：注入 0.01 初始扰动并立即渲染首帧（不静置在 0）。
 * wasm 来源：支持内嵌 base64（双击 file:// 场景）或 fetch（服务器场景）。
 */
(function () {
  var canvas = document.getElementById("scope");
  var ctx = canvas.getContext("2d");
  var spark = document.getElementById("spark");
  var sparkCtx = spark.getContext("2d");

  var api = null;
  var outBuf = [];
  var MAX = 96;
  var tick = 0;
  var lastEnt = 0;

  var REQUIRED_EXPORTS = [
    "push_seed", "pop_result", "get_entropy",
    "get_thinking_len", "get_thinking_nodes",
    "get_diag_formation", "get_diag_resolution", "get_diag_solved"
  ];

  function base64ToBytes(b64) {
    var bin = atob(b64);
    var a = new Uint8Array(bin.length);
    for (var i = 0; i < bin.length; i++) a[i] = bin.charCodeAt(i);
    return a;
  }

  function loadWasm() {
    if (window.NPB_B64) return Promise.resolve(base64ToBytes(window.NPB_B64));
    return fetch("npb.wasm").then(function (r) { return r.arrayBuffer(); })
      .then(function (b) { return new Uint8Array(b); });
  }

  function stat(id, text) { document.getElementById(id).textContent = text; }

  /* 熵 → 色调：ent=0 冷青(190°) ↔ ent=1 暖金(28°)，主圆与波形共用 */
  function tint(ent) {
    var hue = 190 - 162 * ent;
    return { hue: hue, fill: "hsl(" + hue + ",68%,60%)", glow: "hsla(" + hue + ",68%,50%," + (0.10 + ent * 0.16) + ")" };
  }

  function drawWave() {
    var w = spark.width.baseVal.value, h = spark.height.baseVal.value;
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

  function frame() {
    if (!api) return;
    tick++;

    var seed = tick % 24 === 0 ? 1.0 : 0.5 + 0.10 * Math.sin(tick / 9);
    if (tick % 61 === 0) { api.push_seed(0.9); api.push_seed(0.85); } // 成对突发（干涉演示）
    api.push_seed(seed);

    var out = api.pop_result();
    var ent = api.get_entropy();
    var clen = api.get_thinking_len();
    var cnodes = api.get_thinking_nodes();
    var f = api.get_diag_formation();
    var r = api.get_diag_resolution();
    var solved = api.get_diag_solved();
    lastEnt = ent;

    stat("out", out.toFixed(3));
    stat("ent", ent.toFixed(3));
    stat("chainLen", String(clen));
    stat("chainNodes", String(cnodes));
    stat("diag", "形成 " + f + " 步 · 解决 " + r + " 步");
    stat("diagState", solved === 1 ? "已收敛" : "收敛中");

    var cx = canvas.width / 2, cy = canvas.height / 2;
    var radius = 8 + out * 125;
    var c = tint(ent);

    ctx.clearRect(0, 0, canvas.width, canvas.height);
    ctx.beginPath();
    ctx.arc(cx, cy, radius + 20 + ent * 24, 0, Math.PI * 2);
    ctx.fillStyle = c.glow;
    ctx.fill();
    ctx.beginPath();
    ctx.arc(cx, cy, radius, 0, Math.PI * 2);
    ctx.fillStyle = c.fill;
    ctx.fill();
    ctx.beginPath(); // 0 锚点常在
    ctx.arc(cx, cy, 2.5, 0, Math.PI * 2);
    ctx.fillStyle = "#07090d";
    ctx.fill();

    outBuf.push(out);
    if (outBuf.length > MAX) outBuf.shift();
    drawWave();
  }

  loadWasm()
    .then(function (bytes) { return WebAssembly.instantiate(bytes, {}); })
    .then(function (res) {
      api = res.instance.exports;
      var missing = REQUIRED_EXPORTS.filter(function (k) { return typeof api[k] !== "function"; });
      if (missing.length) {
        stat("out", "缺导出: " + missing.join(","));
        throw new Error("missing exports: " + missing.join(","));
      }
      // 加载即呼吸：0 锚点 → 0.01 初始扰动（柔和起搏）→ 1.0 点燃 → 立即渲染首帧
      api.push_seed(0.01);
      api.push_seed(1.0);
      frame();
      setInterval(frame, 120);
    })
    .catch(function (err) {
      if (err && err.message && err.message.indexOf("missing") !== 0) stat("out", "wasm 加载失败");
      console.error(err);
    });

  document.getElementById("seedBtn").addEventListener("click", function () {
    if (api) api.push_seed(1.0);
  });
})();
