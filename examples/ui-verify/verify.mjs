/**
 * 正源浏览器 · 自动目验（UI_AUTO_VERIFY）
 * ============================================================
 * 目的：把「待用户目验」转为「CI 自动验证」。
 *
 * 验证链（端到端，真实浏览器执行 wasm）：
 *   1. 打开页面（网关同源托管 dist/）
 *   2. 等 wasm 初始化并完成一轮诊断渲染（动态内容出现）
 *   3. 断言：标题 / 视图标签 / 四场状态字（亢枯平）/ 确信度 / 溯源 / 多语言正文
 *   4. 截图存档（失败时可回溯）
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

  // ---- 2) 等 wasm 完成一轮诊断渲染（动态内容 = 证明 wasm 真的跑起来了）----
  let rendered = true;
  try {
    await page.waitForFunction(
      () => document.body && document.body.innerText.includes('确信度'),
      null,
      { timeout: TIMEOUT }
    );
  } catch {
    rendered = false;
  }
  check('wasm 已执行并完成诊断渲染', rendered);

  const body = await page.innerText('body');

  // ---- 3) 内容断言 ----
  check('标题为「正源浏览器」', body.includes('正源浏览器'));
  check('四个视图标签齐全',
    ['诊断台', '显化台', '设备台', '设置'].every((t) => body.includes(t)));
  check('出现四场状态字（亢/枯/平）', /[亢枯平]/.test(body));
  check('呈现「确信度」', body.includes('确信度'));
  check('呈现「溯源」', body.includes('溯源'));
  check('呈现「建议」', body.includes('建议'));
  check('呈现「成因」', body.includes('成因'));

  // ---- 4) 关键元素存在（DOM 级，不只文本）----
  check('四场进度条存在（bar-earth/water/fire/wind）',
    (await page.locator('#bar-earth, #bar-water, #bar-fire, #bar-wind').count()) === 4);
  check('诊断台主体容器存在（diag-body）',
    (await page.locator('#diag-body').count()) === 1);
  check('状态带存在（probe-time）',
    (await page.locator('#probe-time').count()) === 1);
  check('结论标题元素非空（d-title）',
    ((await page.locator('#d-title').innerText()) || '').trim().length > 0);

  // ---- 5) 截图存档 ----
  await page.screenshot({ path: SHOT, fullPage: true });
  log('截图:', SHOT, fs.existsSync(SHOT) ? '(ok)' : '(缺失)');
} catch (e) {
  check('未捕获异常', false, String(e && e.message || e));
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
