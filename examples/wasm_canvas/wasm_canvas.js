/*
 * examples/wasm_canvas/wasm_canvas.js
 *
 * 加载 npb.wasm（由 cargo build -p npb --target wasm32-unknown-unknown --release 产出，
 * 复制到本目录），用同一套 bridge 三接口驱动 Canvas 圆点"呼吸"。
 * 所有绘制数值都来自内核输出 pop_result() 与 get_entropy()。
 */
(function () {
  const canvas = document.getElementById("breath");
  const ctx = canvas.getContext("2d");
  const outEl = document.getElementById("out");
  const entEl = document.getElementById("ent");
  const beatEl = document.getElementById("beat");

  let api = null; // wasm 导出
  let beats = 0;
  let tick = 0;

  function loadWasm(url) {
    // 支持内嵌 base64（双击 file:// 场景，见 tools/embed_wasm.py）或 fetch（服务器场景）
    let bytesPromise;
    if (window.NPB_B64) {
      const bin = atob(window.NPB_B64);
      const bytes = new Uint8Array(bin.length);
      for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
      bytesPromise = Promise.resolve(bytes);
    } else {
      bytesPromise = fetch(url).then((r) => r.arrayBuffer());
    }
    return bytesPromise.then((bytes) => WebAssembly.instantiate(bytes, {})).then(({ instance }) => instance.exports);
  }

  // 每个 tick：周期脉冲 + 平稳注入 → 读输出 → 画呼吸圆
  function frame() {
    if (!api) return;
    tick++;

    // 注入：脉冲（每 20 tick 一次）+ 平稳种子
    const seed = tick % 20 === 0 ? 1.0 : 0.5 + 0.08 * Math.sin(tick / 7);
    api.push_seed(seed);

    const out = api.pop_result(); // 内核输出 → 圆的半径
    const ent = api.get_entropy(); // 系统熵 → 呼吸节律/颜色

    outEl.textContent = out.toFixed(3);
    entEl.textContent = ent.toFixed(3);

    const cx = canvas.width / 2;
    const cy = canvas.height / 2;
    const radius = 8 + out * 130;

    // 熵低（有序沉稳）→ 青绿；熵高（混沌）→ 泛紫
    const g = Math.round(150 + 100 * (1 - ent));
    const b = Math.round(120 + 120 * ent);
    ctx.clearRect(0, 0, canvas.width, canvas.height);

    // 外圈：由熵决定晕圈大小
    ctx.beginPath();
    ctx.arc(cx, cy, radius + 18 + ent * 26, 0, Math.PI * 2);
    ctx.fillStyle = `rgba(29,158,117,${0.10 + ent * 0.12})`;
    ctx.fill();

    // 呼吸圆本体
    ctx.beginPath();
    ctx.arc(cx, cy, radius, 0, Math.PI * 2);
    ctx.fillStyle = `rgb(${Math.round(90 + 70 * (1 - ent))}, ${g}, ${b})`;
    ctx.fill();

    // 0 锚点标记：圆心小点（真空常在）
    ctx.beginPath();
    ctx.arc(cx, cy, 2.5, 0, Math.PI * 2);
    ctx.fillStyle = "#0b0e14";
    ctx.fill();

    if (out > 0.7) beats++; // 强搏动计数
    beatEl.textContent = String(beats);
  }

  loadWasm("npb.wasm")
    .then((exports) => {
      api = exports;
      // 点燃：0 锚点后注入第一扰动
      api.push_seed(1.0);
      api.push_seed(1.0); // 双脉冲演示成对干涉路径
      setInterval(frame, 120);
    })
    .catch((err) => {
      outEl.textContent = "wasm 加载失败";
      console.error(err);
    });

  const btn = document.getElementById("seedBtn");
  if (btn) btn.addEventListener("click", () => api && api.push_seed(1.0));
})();
