/*
 * examples/zen-oscilloscope/zen.js
 *
 * 禅境示波器逻辑：WASM 内核直驱。
 * - 圆：半径 = pop_result()（内核输出）；色温/晕圈 = get_entropy()；
 * - 数据区：思考链长度（get_thinking_len）、推演节点数（get_thinking_nodes）、
 *   双链诊断（get_diag_formation / get_diag_resolution / get_diag_solved）；
 * - 底部：输出波形（呼吸痕迹）。
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

  function base64ToBytes(b64) {
    var bin = atob(b64);
    var a = new Uint8Array(bin.length);
    for (var i = 0; i < bin.length; i++) a[i] = bin.charCodeAt(i);
    return a;
  }

  function loadWasm() {
    if (window.NPB_B64) {
      return Promise.resolve(base64ToBytes(window.NPB_B64));
    }
    return fetch("npb.wasm")
      .then(function (r) { return r.arrayBuffer(); })
      .then(function (b) { return new Uint8Array(b); });
  }

  function stat(id, text) { document.getElementById(id).textContent = text; }

  function drawWave() {
    var w = spark.width.baseVal.value, h = spark.height.baseVal.value;
    sparkCtx.clearRect(0, 0, w, h);
    if (outBuf.length < 2) return;
    sparkCtx.beginPath();
    for (var i = 0; i < outBuf.length; i++) {
      var x = (i / (MAX - 1)) * w;
      var y = h - 4 - outBuf[i] * (h - 8);
      if (i === 0) sparkCtx.moveTo(x, y); else sparkCtx.lineTo(x, y);
    }
    sparkCtx.strokeStyle = "rgba(29,158,117,.9)";
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

    stat("out", out.toFixed(3));
    stat("ent", ent.toFixed(3));
    stat("chainLen", String(clen));
    stat("chainNodes", String(cnodes));
    stat("diag", "形成 " + f + " 步 · 解决 " + r + " 步");
    stat("diagState", solved === 1 ? "已收敛" : "收敛中");

    var cx = canvas.width / 2, cy = canvas.height / 2;
    var radius = 8 + out * 125;

    ctx.clearRect(0, 0, canvas.width, canvas.height);
    ctx.beginPath();
    ctx.arc(cx, cy, radius + 20 + ent * 24, 0, Math.PI * 2);
    ctx.fillStyle = "rgba(29,158,117," + (0.08 + ent * 0.12) + ")";
    ctx.fill();
    ctx.beginPath();
    ctx.arc(cx, cy, radius, 0, Math.PI * 2);
    var g = Math.round(150 + 90 * (1 - ent));
    var b = Math.round(110 + 130 * ent);
    ctx.fillStyle = "rgb(" + Math.round(80 + 80 * (1 - ent)) + "," + g + "," + b + ")";
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
    .then(function (bytes) {
      return WebAssembly.instantiate(bytes, {});
    })
    .then(function (res) {
      api = res.instance.exports;
      api.push_seed(1.0); // 第一扰动点燃
      setInterval(frame, 120);
    })
    .catch(function (err) {
      stat("out", "wasm 加载失败");
      console.error(err);
    });

  document.getElementById("seedBtn").addEventListener("click", function () {
    if (api) api.push_seed(1.0);
  });
})();
