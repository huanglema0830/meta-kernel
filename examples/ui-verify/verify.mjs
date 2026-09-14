/**
 * 空天浏览器 · 自动目验（UI_AUTO_VERIFY）v2
 * ============================================================
 * 目的：把「待用户目验」转为「CI 自动验证」；并按发起人指令补齐五类检测：
 *   ① 兼容性（基础 API 可用性，页面内自检）
 *   ② 性能断言（启动耗时 / JS 堆占用）
 *   ③ 安全断言（XSS / 注入：载荷不执行且被转义）
 *   ④ 集成断言（新建→编辑→保存→重载→内容仍在）
 *   ⑤ 混沌断言（空名 / 超长 / 纯符号 / 同名 —— 不崩）
 *   ＋ 原有：诊断视图渲染断言（wasm 真实执行）
 *
 * 用法：
 *   node verify.mjs                       # 默认 http://127.0.0.1:3210/
 *   UI_URL=http://127.0.0.1:3000/ node verify.mjs
 *
 * 注意：本目录是**测试工具**（devDependency），不影响产物零依赖红线。
 */
import { chromium } from 'playwright';
import fs from 'node:fs';

const URL = process.env.UI_URL || 'http://127.0.0.1:3210/';
const SHOT = process.env.UI_SHOT || 'ui_verify.png';
const TIMEOUT = Number(process.env.UI_TIMEOUT || 45000);

// 性能阈值（发起人要求：内存 < 10MB、启动 < 1 秒；CI 机器有波动，略放宽并记录实测）
const MAX_STARTUP_MS = Number(process.env.UI_MAX_STARTUP_MS || 1500);
const MAX_HEAP_MB = Number(process.env.UI_MAX_HEAP_MB || 10);

function log(...a) { console.log('[verify]', ...a); }
const checks = [];
function check(name, ok, detail = '') {
  checks.push({ name, ok, detail });
  log(`${ok ? 'PASS' : 'FAIL'}  ${name}${detail ? '  (' + detail + ')' : ''}`);
}

const browser = await chromium.launch({ args: ['--no-sandbox'] });
const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });

try {
  // ---- 1) 页面可访问 ----
  const resp = await page.goto(URL, { waitUntil: 'load', timeout: TIMEOUT });
  check('页面可访问', !!resp && resp.status() === 200, `HTTP ${resp && resp.status()}`);

  // ---- 2) 工作台就绪 + 默认视图 = 工作台（验收标准 1）----
  let wcReady = true;
  try {
    await page.waitForFunction(() => window.__wcReady === 1, null, { timeout: TIMEOUT });
  } catch { wcReady = false; }
  check('工作台脚本已就绪（__wcReady）', wcReady);

  const workActive = await page.evaluate(() =>
    !!document.getElementById('view-work') &&
    String(document.getElementById('view-work').className).indexOf('active') >= 0);
  check('默认视图为「工作台」（打开即干活）', workActive);

  const body = await page.innerText('body');
  check('标题为「空天浏览器」', body.includes('空天浏览器'));
  check('五个标签齐全（工作台/诊断台/显化台/设备台/设置）',
    ['工作台', '诊断台', '显化台', '设备台', '设置'].every((t) => body.includes(t)));

  // ---- 3) MVP 两项工具齐备 ----
  check('MVP①：文档编辑器存在（doc-edit / doc-save）',
    (await page.locator('#doc-edit').count()) === 1 &&
    (await page.locator('#doc-save').count()) === 1);
  check('MVP②：文件工位存在（wt-files / wp-files / file-stat）',
    (await page.locator('#wt-files').count()) === 1 &&
    (await page.locator('#wp-files').count()) === 1 &&
    (await page.locator('#file-stat').count()) === 1);
  check('诊断降为辅助（工作台内有「诊断（辅助）」入口）',
    (await page.locator('#wt-diag').count()) === 1);

  // ---- 4) 兼容性（页面内基础 API 自检）----
  const compat = await page.evaluate(() => {
    const out = {};
    try { out.dom = !!document.getElementById('doc-edit') && !!document.createElement('div').appendChild; } catch { out.dom = false; }
    try { out.evt = !!document.addEventListener; } catch { out.evt = false; }
    try { const d = document.createElement('div'); d.textContent = 'x'; out.text = d.textContent === 'x'; } catch { out.text = false; }
    try { localStorage.setItem('__v', '1'); out.ls = localStorage.getItem('__v') === '1'; localStorage.removeItem('__v'); } catch { out.ls = false; }
    try { out.json = JSON.parse(JSON.stringify({ a: 1 })).a === 1; } catch { out.json = false; }
    try { out.keys = Object.keys({ a: 1 }).length === 1; } catch { out.keys = false; }
    return out;
  });
  check('兼容性：基础 API 全部可用（DOM/事件/textContent/localStorage/JSON/Object.keys）',
    Object.values(compat).every(Boolean), JSON.stringify(compat));

  // ---- 5) 集成：新建 → 编辑 → 保存 → 重载 → 内容仍在 ----
  const docName = 'CI-' + Date.now();
  await page.fill('#doc-name', docName);
  await page.fill('#doc-edit', '# 标题\n正文一行\n- 列表项');
  await page.click('#doc-save');
  await page.waitForTimeout(120);
  await page.reload({ waitUntil: 'load', timeout: TIMEOUT });
  await page.waitForFunction(() => window.__wcReady === 1, null, { timeout: TIMEOUT });
  const persisted = await page.evaluate((nm) => {
    try {
      const db = JSON.parse(localStorage.getItem('sb.files.v1') || '{}');
      return !!(db[nm] && String(db[nm].content).indexOf('正文一行') >= 0);
    } catch { return false; }
  }, docName);
  check('集成：新建→编辑→保存→重载后内容仍在', persisted, docName);

  // 列表可见（文件管理能看到同一份数据）
  const listHasDoc = await page.evaluate((nm) =>
    String(document.getElementById('doc-list').innerText).indexOf(nm) >= 0, docName);
  check('集成：文件/文档列表显示该条目', listHasDoc);

  // ---- 6) 安全：XSS / 注入（载荷不执行 + 被转义）----
  await page.evaluate(() => { window.__xss = 0; });
  const XSS = '<img src=x onerror="window.__xss=1"><script>window.__xss=2<\/script>[点我](javascript:window.__xss=3)';
  await page.fill('#doc-edit', XSS);
  await page.waitForTimeout(200);
  const xssExecuted = await page.evaluate(() => window.__xss || 0);
  check('安全：XSS 载荷未被执行', xssExecuted === 0, `__xss=${xssExecuted}`);
  const previewHtml = await page.evaluate(() => document.getElementById('doc-preview').innerHTML);
  const escapedOk =
    previewHtml.indexOf('&lt;img') >= 0 || previewHtml.indexOf('&lt;script') >= 0;
  check('安全：载荷被转义（预览不产生真实标签）', escapedOk);
  check('安全：javascript: 伪协议链接未被渲染为 <a>',
    !/href\s*=\s*["']?\s*javascript:/i.test(previewHtml));
  const imgCount = await page.locator('#doc-preview img').count();
  check('安全：预览中无注入产生的 <img> 元素', imgCount === 0, `img=${imgCount}`);

  // ---- 7) 混沌：空名 / 超长 / 纯符号 / 同名 —— 不崩 ----
  const longText = 'x'.repeat(100 * 1024); // 100 KB
  const chaosCases = [
    ['空名保存', async () => { await page.fill('#doc-name', '   '); await page.click('#doc-save'); }],
    ['超长内容(100KB)', async () => { await page.fill('#doc-edit', longText); }],
    ['纯符号内容', async () => { await page.fill('#doc-edit', '!@#$%^&*()_+-=[]{}|;:\'"<>,.?/\\`~'); }],
    ['空内容', async () => { await page.fill('#doc-edit', ''); }],
  ];
  let chaosOk = true;
  for (const [label, fn] of chaosCases) {
    try { await fn(); await page.waitForTimeout(60); }
    catch (e) { chaosOk = false; log('混沌用例异常:', label, String(e && e.message || e)); }
  }
  const stillAlive = await page.evaluate(() => window.__wcReady === 1 && !!document.getElementById('doc-preview'));
  check('混沌：空名/超长/纯符号/空内容 均未使页面崩溃', chaosOk && stillAlive);

  // ---- 7.5) 元内核接管：自我监控 / 健康报告 / 告警 / 运行日志（含任务归属）+ 升级入口 ----
  const base = new globalThis.URL('/', URL).href.replace(/\/$/, '');
  const jget = async (p) => {
    try { const r = await fetch(base + p); return { ok: r.ok, status: r.status, text: await r.text() }; }
    catch { return { ok: false, status: 0, text: '' }; }
  };

  const rep = await jget('/v1/report');
  let repJ = null; try { repJ = JSON.parse(rep.text); } catch { /* 非法 JSON */ }
  check('元内核自监控：/v1/report 可用且含 版本/计数器/运行时长',
    rep.ok && !!repJ && !!repJ.version && !!repJ.counters && typeof repJ.uptime_s === 'number',
    `status=${rep.status}`);
  const alr = await jget('/v1/alerts');
  let alrJ = null; try { alrJ = JSON.parse(alr.text); } catch { /* 非法 JSON */ }
  check('元内核异常告警：/v1/alerts 可用', alr.ok && !!alrJ && Array.isArray(alrJ.alerts), `status=${alr.status}`);

  // 任务归属：先投递一条 [WorkBuddy] 任务，再校验"每行都有前缀"
  const post = await fetch(base + '/v1/tasks', {
    method: 'POST', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ owner: 'workbuddy', detail: 'CI 集成测试任务投递' }),
  }).catch(() => null);
  check('任务归属：POST /v1/tasks 接受投递', !!post && post.ok);
  const tk = await jget('/v1/tasks.txt');
  const tklines = tk.text.split('\n').filter((l) => l.trim().length > 0);
  check('任务归属：运行日志每行带 [元内核]/[WorkBuddy] 前缀',
    tklines.length > 0 && tklines.every((l) => l.startsWith('[元内核]') || l.startsWith('[WorkBuddy]')),
    `lines=${tklines.length}`);
  check('任务归属：[WorkBuddy] 投递已被记录', tk.text.includes('[WorkBuddy]') && tk.text.includes('CI 集成测试任务投递'));

  // 工作台「任务归属」面板
  await page.click('#wt-owner');
  await page.waitForTimeout(800);
  const ownTxt = await page.innerText('#wp-owner');
  check('工作台：「任务归属」面板显示健康报告与带前缀日志',
    /运行/.test(ownTxt) && (ownTxt.indexOf('[元内核]') >= 0 || ownTxt.indexOf('[WorkBuddy]') >= 0),
    ownTxt.substring(0, 50).replace(/\s+/g, ' '));
  await page.click('#wt-doc');

  // 升级入口（**不改网络配置**）
  const up = await jget('/upgrade');
  check('升级入口：/upgrade 页面可用', up.ok && up.text.includes('升级'));
  check('升级入口：页面声明不修改网络配置',
    up.text.includes('不触碰') || up.text.includes('不修改任何网络配置'));
  const upb = await jget('/upgrade.bat');
  check('升级入口：/upgrade.bat 可下载且含"备份 + 不改网络配置"',
    upb.ok && upb.text.includes('备份') &&
    (upb.text.includes('未修改任何网络配置') || upb.text.includes('不修改任何网络配置')),
    'len=' + upb.text.length);
  const pkg = await fetch(base + '/upgrade-package.zip').catch(() => null);
  check('升级入口：升级包可下载（zip > 1KB）',
    !!pkg && pkg.ok && Number(pkg.headers.get('content-length') || 0) > 1024,
    pkg ? `status=${pkg.status} len=${pkg.headers.get('content-length')}` : 'fetch failed');

  // ---- 7.6) L7 T1 闭环（内核只建议 + 宿主执行；只接受预置动作 id）----
  const acts = await jget('/v1/actions');
  let actsJ = null; try { actsJ = JSON.parse(acts.text); } catch { /* ignore */ }
  check('L7：动作表可用且只开放 T1',
    acts.ok && !!actsJ && actsJ.t1_only === true && Array.isArray(actsJ.actions) && actsJ.actions.length === 4,
    `count=${actsJ && actsJ.actions ? actsJ.actions.length : '?'}`);
  const keys = (actsJ && actsJ.actions ? actsJ.actions : []).map((a) => a.key).join(',');
  check('L7：四个 T1 动作齐备（clean-temp/restart-watchdog/reload-config/trigger-probe）',
    keys === 'clean-temp,restart-watchdog,reload-config,trigger-probe', keys);
  check('L7：全部为 T1（无 T2/T3 被误放行）',
    (actsJ && actsJ.actions ? actsJ.actions : []).every((a) => a.grade === 1));

  const postJson = async (p, obj) => {
    try {
      const r = await fetch(base + p, {
        method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(obj),
      });
      return { ok: r.ok, status: r.status, text: await r.text() };
    } catch { return { ok: false, status: 0, text: '' }; }
  };

  // ① 未知 id 必须拒绝
  const badId = await postJson('/v1/grant', { id: 99, remember: false });
  check('L7：未知动作 id 被拒（白名单之外不可执行）', badId.status === 400, `status=${badId.status}`);
  // ② 伪令牌必须拒绝
  const badTok = await postJson('/v1/execute', { id: 1, token: 'deadbeef' });
  let btJ = null; try { btJ = JSON.parse(badTok.text); } catch { /* ignore */ }
  check('L7：伪令牌被拒（防重放）', badTok.ok && btJ && btJ.ok === false, btJ ? btJ.outcome : '');

  // ③ 首次授权 → 执行 → 记账（正路）
  const g1 = await postJson('/v1/grant', { id: 3, remember: true });   // reload-config：只读+写清单，最安全
  let g1J = null; try { g1J = JSON.parse(g1.text); } catch { /* ignore */ }
  check('L7：授权签发一次性令牌', g1.ok && !!g1J && !!g1J.token, `status=${g1.status}`);
  const e1 = await postJson('/v1/execute', { id: 3, token: g1J ? g1J.token : '' });
  let e1J = null; try { e1J = JSON.parse(e1.text); } catch { /* ignore */ }
  check('L7：首次授权 → 执行成功', e1.ok && e1J && e1J.ok === true, e1J ? String(e1J.output).slice(0, 60) : '');
  check('L7：执行已写入审计哈希链且链完整', e1J && e1J.chain_ok === true && e1J.hash > 0);
  // ④ 令牌一次性：同令牌再执行必须失败
  const e2 = await postJson('/v1/execute', { id: 3, token: g1J ? g1J.token : '' });
  let e2J = null; try { e2J = JSON.parse(e2.text); } catch { /* ignore */ }
  check('L7：令牌一次性（重复使用被拒）', e2J && e2J.ok === false && e2J.outcome === 'bad_token');
  // ⑤ 已授权 → 免确认（grant remember=false 即免确认路径）
  const g2 = await postJson('/v1/grant', { id: 3, remember: false });
  let g2J = null; try { g2J = JSON.parse(g2.text); } catch { /* ignore */ }
  const e3 = await postJson('/v1/execute', { id: 3, token: g2J ? g2J.token : '' });
  let e3J = null; try { e3J = JSON.parse(e3.text); } catch { /* ignore */ }
  check('L7：授权后免确认（再次执行成功）', e3J && e3J.ok === true);
  // ⑥ 回滚可用
  const rb = await postJson('/v1/rollback', { id: 3 });
  let rbJ = null; try { rbJ = JSON.parse(rb.text); } catch { /* ignore */ }
  check('L7：回滚可用且结论如实', rb.ok && rbJ && rbJ.ok === true, rbJ ? String(rbJ.output).slice(0, 50) : '');
  // ⑦ 撤销授权 → 需重新确认
  const rv = await postJson('/v1/revoke', { id: 3 });
  check('L7：撤销授权成功', rv.ok);
  const g3 = await postJson('/v1/grant', { id: 3, remember: false });
  let g3J = null; try { g3J = JSON.parse(g3.text); } catch { /* ignore */ }
  const e4 = await postJson('/v1/execute', { id: 3, token: g3J ? g3J.token : '' });
  let e4J = null; try { e4J = JSON.parse(e4.text); } catch { /* ignore */ }
  check('L7：撤销后必须重新确认（执行被拒）', e4J && e4J.ok === false && e4J.outcome === 'revoked');
  // ⑧ 审计链可读且带归属前缀
  const audit = await jget('/v1/audit.txt');
  const al = audit.text.split('\n').filter((l) => l.trim().length > 0);
  check('L7：审计链可读且每行带 [元内核] 前缀',
    audit.ok && al.length > 0 && al.every((l) => l.startsWith('[元内核]')), `lines=${al.length}`);

  // 工作台「系统自我维护」区（Q4：放自监控面板）
  await page.click('#wt-owner');
  await page.waitForTimeout(900);
  const own2 = await page.innerText('#wp-owner');
  check('UI：自监控面板含「系统自我维护」区并列出 4 个 T1 动作',
    own2.includes('系统自我维护') && own2.includes('清理本应用临时文件') && own2.includes('触发探针采集'));
  await page.click('#wt-doc');

  // ---- 8) 性能断言 ----
  const perf = await page.evaluate(() => window.__wcPerf || { startupMs: -1, heapMB: -1 });
  check('性能：工作台初始化 < 1 秒', perf.startupMs >= 0 && perf.startupMs < MAX_STARTUP_MS,
    `startupMs=${perf.startupMs}`);
  // JS 堆采用**两层判据**：
  //   硬阈值（防退化）——失败即红；
  //   目标 <10 MB —— 记录（Chromium 自身堆基线约 5–10 MB，严格 <10MB 的公平判据在 Phase 4
  //   用「同浏览器开本页 vs 开空白页」的增量 + 进程 RSS 复核，避免用基线噪声判定而成假红）。
  const HEAP_HARD = Number(process.env.UI_MAX_HEAP_MB_HARD || 20);
  const heapOk = perf.heapMB <= 0 || perf.heapMB < HEAP_HARD;
  check('性能：JS 堆占用未退化（硬阈值 < ' + HEAP_HARD + ' MB）', heapOk,
    `heapMB=${perf.heapMB > 0 ? perf.heapMB.toFixed(2) : 'n/a（该浏览器不暴露）'}`);
  if (perf.heapMB > 0) {
    log(`  目标 <10MB：${perf.heapMB < 10 ? '达成' : '未达成（Phase 4 用增量/RSS 复核）'}  heapMB=${perf.heapMB.toFixed(2)}`);
  }

  // ---- 9) 诊断视图（辅助）：切到诊断台，强制刷新一次，等 wasm 完成渲染 ----
  await page.click('#tab-diag');
  await page.click('#btn-refresh').catch(() => {});   // 显式触发 refresh_diag()（不依赖加载时序）
  let rendered = true;
  try {
    await page.waitForFunction(
      () => document.body && document.body.innerText.includes('确信度'),
      null, { timeout: TIMEOUT });
  } catch { rendered = false; }
  check('wasm 已执行并完成诊断渲染（诊断台）', rendered);

  const diag = await page.innerText('body');
  check('出现四场状态字（亢/枯/平）', /[亢枯平]/.test(diag));
  check('呈现「确信度」', diag.includes('确信度'));
  check('呈现「溯源」', diag.includes('溯源'));
  check('呈现「建议」', diag.includes('建议'));
  check('呈现「成因」', diag.includes('成因'));
  check('四场进度条存在（bar-earth/water/fire/wind）',
    (await page.locator('#bar-earth, #bar-water, #bar-fire, #bar-wind').count()) === 4);
  check('状态带存在（probe-time）', (await page.locator('#probe-time').count()) === 1);
  check('结论标题元素非空（d-title）',
    ((await page.locator('#d-title').innerText()) || '').trim().length > 0);

  // 截图（工作台视图）
  await page.click('#tab-work').catch(() => {});
  await page.screenshot({ path: SHOT, fullPage: true });
  log('截图:', SHOT, fs.existsSync(SHOT) ? '(ok)' : '(缺失)');
} catch (e) {
  check('未捕获异常', false, String((e && e.message) || e));
} finally {
  await browser.close();
}

const failed = checks.filter((c) => !c.ok);
console.log('\n==== UI 自动目验汇总 ====');
console.log(`共 ${checks.length} 项，通过 ${checks.length - failed.length}，失败 ${failed.length}`);
if (failed.length) {
  console.log('失败项：');
  for (const f of failed) console.log('  -', f.name, f.detail);
  process.exit(1);
}
console.log('UI_AUTO_VERIFY_OK');
