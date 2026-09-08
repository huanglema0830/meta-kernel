// Manifest Journal UI — 应用逻辑：连网关、推送、订阅、驱动生命周期、渲染（浏览器）。
(function () {
  'use strict';
  const GW_DEFAULT = 'http://127.0.0.1:3000';

  const $ = (id) => document.getElementById(id);
  const el = {
    status: $('gw-status'), gwInput: $('gw-addr'), connectBtn: $('btn-connect'),
    raw: $('input-raw'), lightBtn: $('btn-light'),
    card: $('entry-card'), cardRaw: $('card-raw'), cardSeed: $('card-seed'),
    life: $('life-state'), lifeBand: $('life-band'), rounds: $('life-rounds'),
    gauge: $('gauge-fill'), log: $('log-list'), intent: $('intent-text'),
    boostBtn: $('btn-boost'), archiveBtn: $('btn-archive'),
  };

  let gw = localStorage.getItem('mj_gw') || GW_DEFAULT;
  let entry = null;          // {raw, seed, id}
  let eng = Lifecycle.newEngine();
  let es = null;             // EventSource
  const logBuf = [];

  function setStatus(txt, ok) {
    el.status.textContent = txt;
    el.status.className = 'status ' + (ok ? 'ok' : 'bad');
  }

  async function api(path, opts) {
    const r = await fetch(gw + path, opts);
    const t = await r.text();
    return t;
  }

  // 确定性种子（FNV-1a 风格 → [0.25,0.95]，与 Rust seed_of 同分布策略）
  function seedOf(text) {
    let h = 0xcbf29ce484222325n;
    for (const b of new TextEncoder().encode(text)) {
      h ^= BigInt(b);
      h = (h * 0x100000001b3n) & 0xFFFFFFFFFFFFFFFFn;
    }
    const mix = Number((h >> 32n) ^ (h & 0xFFFFFFFFn));
    const x = (mix >>> 0) / 0xFFFFFFFF;
    return 0.25 + 0.70 * x;
  }

  async function push(seed) {
    const r = await api('/v1/push', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ seed: Number(seed.toFixed(4)) }),
    });
    return r.includes('"accepted":true');
  }

  async function refreshState() {
    try {
      const t = await api('/v1/state');
      const j = JSON.parse(t.slice(t.indexOf('{')));
      el.status.textContent = '网关在线 · t=' + j.t + ' stored=' + Number(j.energy.stored).toFixed(3);
      el.status.className = 'status ok';
      return true;
    } catch (e) { setStatus('网关不可达：' + e.message, false); return false; }
  }

  function subscribe() {
    if (es) es.close();
    es = new EventSource(gw + '/v1/events');
    es.onopen = () => setStatus('已订阅 /v1/events', true);
    es.onerror = () => setStatus('订阅中断（网关离线？）', false);
    es.addEventListener('state_change', (e) => handleEv('state_change', e.data));
    es.addEventListener('instruction', (e) => handleEv('instruction', e.data));
    es.addEventListener('snapshot', (e) => handleEv('snapshot', e.data));
  }

  function handleEv(kind, data) {
    const ev = Lifecycle.normalize(kind, data);
    if (kind === 'snapshot') return; // 仅作保活
    const before = eng.state;
    Lifecycle.apply(eng, ev);
    if (eng.state !== before || kind === 'instruction') {
      renderEntry();
      const name = Lifecycle.bandName(eng.state);
      const note = kind === 'instruction'
        ? ('指令 · ' + data.slice(0, 60))
        : (before + ' → ' + eng.state + '（' + name + '）');
      pushLog(kind + (before !== eng.state ? ' · ' + note : ''), eng.state);
    }
  }

  function pushLog(text, state) {
    logBuf.unshift({ text, state, at: new Date().toLocaleTimeString() });
    if (logBuf.length > 8) logBuf.pop();
    renderLog();
  }

  function renderLog() {
    el.log.innerHTML = logBuf.map((l) =>
      '<li><span class="t">' + l.at + '</span> <b>[' + Lifecycle.bandName(l.state) + ']</b> ' + l.text + '</li>'
    ).join('') || '<li class="dim">（暂无事件——点亮一条念头后此处会出现显化日志）</li>';
  }

  function renderEntry() {
    if (!entry) { el.card.style.display = 'none'; return; }
    el.card.style.display = 'block';
    el.cardRaw.textContent = entry.raw;                 // 原文零修改
    el.cardSeed.textContent = entry.id + ' · seed=' + entry.seed.toFixed(4);
    el.lifeBand.textContent = Lifecycle.bandName(eng.state);
    el.life.className = 'life ' + (eng.state === 0 ? 'void' : 'lit');
    el.life.textContent = String(eng.state).padStart(2, '0');
    el.rounds.textContent = '轮次 ' + eng.rounds + (entry.early ? ' · 早退' + entry.early : '');
    const pct = eng.state === 0 ? 0 : Math.min(100, Math.round((eng.state / 99) * 100));
    el.gauge.style.width = pct + '%';
    // 意图建议
    const hint = eng.state === 99 ? '极显圆满——可归档本轮，等待回融'
      : eng.state === 0 ? '点击「点亮」注入这条念头'
      : '储备推进中——可「注入补充扰动」延续显化，或「归档」早退回融';
    el.intent.textContent = hint;
  }

  async function light() {
    const raw = el.raw.value.trim();
    if (!raw) return;
    const seed = seedOf(raw);
    const id = 'm-' + (Math.floor(seed * 1e9) & 0xffffffff).toString(16);
    entry = { raw, seed, id, early: 0 };
    eng = Lifecycle.newEngine();
    logBuf.length = 0;
    renderEntry(); renderLog();
    const ok = await push(seed);
    if (!ok) { setStatus('push 被拒（网关入口拒绝）', false); return; }
    pushLog('点亮：注入扰动 seed=' + seed.toFixed(4), 0);
    renderEntry();
  }

  async function boost() {
    if (!entry) return;
    const seed = Math.min(0.95, entry.seed + 0.05);
    const ok = await push(seed);
    if (ok) pushLog('补充扰动 seed=' + seed.toFixed(4), eng.state);
    renderEntry();
  }

  function archive() {
    if (!entry) return;
    Lifecycle.apply(eng, 'Reset');
    entry.early = (entry.early || 0) + 1;
    pushLog('归档：早退回融 0 锚点', 0);
    renderEntry();
  }

  async function connect() {
    gw = el.gwInput.value.trim() || GW_DEFAULT;
    localStorage.setItem('mj_gw', gw);
    const up = await refreshState();
    if (up) subscribe();
  }

  // 初始化
  el.gwInput.value = gw;
  el.connectBtn.onclick = connect;
  el.lightBtn.onclick = light;
  el.boostBtn.onclick = boost;
  el.archiveBtn.onclick = archive;
  el.raw.addEventListener('keydown', (e) => { if (e.key === 'Enter') light(); });
  renderEntry(); renderLog();
  connect();
  window.__mj = { seedOf, eng: () => eng, entry: () => entry }; // 调试口（seed 可重放）
})();
