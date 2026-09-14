/**
 * 工作台纯函数测试（逻辑级 · 无需浏览器）
 * ============================================================
 * 目的：把「安全（XSS/注入）」与「健壮（混沌输入）」这两条最关键路径，
 *       做成**不依赖浏览器**的快速断言——即使浏览器目验不可用，安全底线仍被守住。
 *
 * 做法：从 manifest-ui/index.html 抽出 esc / inlineMd / renderMd / safeName 四个纯函数，
 *       构造后直接跑断言（不引入任何依赖）。
 *
 * 用法：node logic-test.mjs
 */
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const INDEX = path.resolve(here, '..', '..', 'manifest-ui', 'index.html');

let html;
try {
  html = fs.readFileSync(INDEX, 'utf8');
} catch (e) {
  console.error('无法读取', INDEX, String(e && e.message || e));
  process.exit(1);
}

const start = html.indexOf('function esc(');
const end = html.indexOf('/* ---------- 工具函数 ---------- */');
if (start < 0 || end < 0) {
  console.error('FAIL: 未在 index.html 中定位到纯函数区（结构变更？）');
  process.exit(1);
}
const code = html.substring(start, end);
const M = new Function(
  code + '\nreturn { esc: esc, inlineMd: inlineMd, renderMd: renderMd, safeName: safeName };'
)();

let pass = 0;
let fail = 0;
const checks = [];
function t(name, cond, extra = '') {
  checks.push({ name, ok: !!cond, extra });
  if (cond) { pass++; console.log('[logic] PASS ', name); }
  else { fail++; console.log('[logic] FAIL ', name, extra); }
}

// ---- 1) esc 基础 ----
t('esc 转义 < > & " \'', M.esc('<a>&"\'') === '&lt;a&gt;&amp;&quot;&#39;', M.esc('<a>&"\''));
t('esc 处理 null/undefined', M.esc(null) === '' && M.esc(undefined) === '');

// ---- 2) XSS 载荷不得产生标签 ----
const payloads = [
  '<script>alert(1)</script>',
  '<img src=x onerror=alert(1)>',
  '<svg/onload=alert(1)>',
  '<iframe src=javascript:alert(1)>',
  '"><script>alert(1)</script>',
  "';alert(1);//",
];
for (const p of payloads) {
  const out = M.renderMd(p);
  const noTag = out.indexOf('<script') < 0 && out.indexOf('<img') < 0 &&
                out.indexOf('<svg') < 0 && out.indexOf('<iframe') < 0;
  t('XSS 转义为纯文本: ' + p.substring(0, 20), noTag, out.substring(0, 70));
}

// ---- 3) 伪协议不得成为链接 ----
t('javascript: 不渲染为 <a href>', M.renderMd('[x](javascript:window.x=1)').indexOf('<a') < 0);
t('data: 不渲染为 <a href>', M.renderMd('[x](data:text/html,<script>1</script>)').indexOf('<a') < 0);
t('https 链接正常渲染', M.renderMd('[官网](https://example.com/a)').indexOf('<a href="https://example.com/a"') >= 0);

// ---- 4) Markdown 基本渲染 ----
const md = M.renderMd('# 标题\n**粗** 与 *斜* 与 `码`\n- 一\n- 二');
t('渲染标题', md.indexOf('<div class="dt">标题</div>') >= 0);
t('渲染加粗/斜体/代码', md.indexOf('<b>粗</b>') >= 0 && md.indexOf('<i>斜</i>') >= 0 && md.indexOf('<code>码</code>') >= 0);
t('渲染列表', md.indexOf('<ul>') >= 0 && md.indexOf('<li>一</li>') >= 0);

// ---- 5) 混沌输入 ----
const chaos = [
  ['空串', ''], ['纯空白', '   \n\t  '], ['纯符号', '!@#$%^&*()_+-=[]{}|;:\'"<>,.?/\\`~'],
  ['超长 200KB', 'x'.repeat(200 * 1024)], ['只有换行', '\n\n\n'], ['null', null],
  ['undefined', undefined], ['未闭合标签', '<b 未闭合'], ['未闭合链接', '[a](http'], ['嵌套引号', '"""\'\'\''],
];
for (const [label, v] of chaos) {
  let ok = true;
  try { ok = typeof M.renderMd(v) === 'string'; } catch { ok = false; }
  t('混沌不崩: ' + label, ok);
}

// ---- 6) safeName ----
t('safeName 去空白', M.safeName('  a b  ') === 'a b');
t('safeName 禁路径分隔符', M.safeName('../../etc/passwd').indexOf('/') < 0);
t('safeName 长度 <=60', M.safeName('x'.repeat(200)).length === 60);
t('safeName 空输入安全', M.safeName(null) === '' && M.safeName(undefined) === '');

console.log(`\n==== 工作台纯函数测试汇总 ====\n共 ${checks.length} 项，通过 ${pass}，失败 ${fail}`);
if (fail) {
  console.log('失败项：');
  for (const c of checks.filter((x) => !x.ok)) console.log('  -', c.name, c.extra);
  process.exit(1);
}
console.log('WORKBENCH_LOGIC_OK');
