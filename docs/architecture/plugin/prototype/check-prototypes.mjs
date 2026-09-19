// ============================================================================
// 插件原型的本地检查（可选，不接 CI；需要 node >= 18）
// ----------------------------------------------------------------------------
// 用法：  node check-prototypes.mjs
//
// 对每个 *.html 做三件事：
//   ① 语法：抽出 <script> 交 vm.Script 解析 —— 抓未闭合字符串/括号
//   ② 加载：用最小 DOM 桩真跑一遍 —— 抓"启动即抛"（取不到元素、拼错 id、握手炸掉）
//   ③ 交互：按每个原型写好的探针点几下 —— 抓"点了没反应"（data-* 名拼错、分支走空）
//
// 它**不**验证外观。视觉只能人眼过；尺寸与色值的权威仍是设计文档与 ui.rs。
// ============================================================================
import { readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import vm from "node:vm";

const HERE = dirname(fileURLToPath(import.meta.url));

/* ---------------------------------------------------------------- DOM 桩 ---- */
const makeCtx2d = () => new Proxy({}, { get: (t, k) => (k in t ? t[k] : () => {}), set: (t, k, v) => { t[k] = v; return true; } });

function makeEl(tag = "div") {
  const listeners = new Map();
  const el = {
    tagName: tag.toUpperCase(),
    id: "", className: "", dataset: {},
    style: { cssText: "", setProperty() {} },
    children: [], parentNode: null,
    value: "", checked: false, disabled: false,
    textContent: "",
    scrollTop: 0, scrollHeight: 0, offsetWidth: 210, offsetHeight: 40,
    classList: {
      _s: new Set(),
      add(c) { this._s.add(c); }, remove(c) { this._s.delete(c); },
      toggle(c, on) { on === undefined ? (this._s.has(c) ? this._s.delete(c) : this._s.add(c)) : (on ? this._s.add(c) : this._s.delete(c)); },
      contains(c) { return this._s.has(c); },
    },
    appendChild(c) { this.children.push(c); c.parentNode = this; return c; },
    removeChild(c) { this.children = this.children.filter(x => x !== c); return c; },
    remove() { if (this.parentNode) this.parentNode.removeChild(this); },
    get childElementCount() { return this.children.length; },
    get firstChild() { return this.children[0] || null; },
    addEventListener(type, fn) { if (!listeners.has(type)) listeners.set(type, []); listeners.get(type).push(fn); },
    dispatch(type, ev = {}) { for (const fn of listeners.get(type) || []) fn(fakeEv(this, ev)); },
    querySelector() { return makeEl("div"); },
    querySelectorAll() { return []; },
    // 探针里最常用的两件事：把事件目标当成"元素自身"处理
    matches() { return true; },
    closest() { return this; },
    getBoundingClientRect() { return { left: 0, top: 0, right: 640, bottom: 300, width: 640, height: 300 }; },
    getContext() { return makeCtx2d(); },
    setAttribute() {}, removeAttribute() {}, click() {}, focus() {}, scrollIntoView() {},
    toDataURL() { return "data:image/png;base64,stub"; },
  };

  // innerHTML 近似浏览器语义：**赋值会替换子节点**。
  // 不这么做，`el.innerHTML = ""` 清不掉 children，所有「消失类」断言都会假通过。
  let _html = "";
  Object.defineProperty(el, "innerHTML", {
    get() { return _html; },
    set(v) { _html = String(v); el.children.length = 0; },
  });

  return el;
}

const fakeEv = (target, extra = {}) => ({
  target, currentTarget: target, preventDefault() {}, stopPropagation() {},
  clientX: 100, clientY: 120, ...extra,
});

function installDom() {
  const byId = new Map();
  const docListeners = new Map();
  const doc = {
    documentElement: Object.assign(makeEl("html"), { setAttribute() {} }),
    body: makeEl("body"),
    createElement: (t) => makeEl(t),
    createTextNode: () => makeEl("span"),
    getElementById(id) {
      if (!byId.has(id)) { const el = makeEl("div"); el.id = id; byId.set(id, el); }
      return byId.get(id);
    },
    querySelector: () => makeEl("div"),
    querySelectorAll: () => [],
    addEventListener(type, fn) { if (!docListeners.has(type)) docListeners.set(type, []); docListeners.get(type).push(fn); },
    removeEventListener() {},
  };

  // 捕获宿主投喂（真实现里是 WebView::evaluate_script）
  const frames = [];
  const win = {
    innerWidth: 1600, innerHeight: 900, devicePixelRatio: 1,
    addEventListener() {}, removeEventListener() {},
  };
  Object.defineProperty(win, "__rds_receive", {
    configurable: true,
    get() { return this._fn; },
    set(fn) { this._fn = (json) => { frames.push(json); return fn(json); }; },
  });

  const style = { getPropertyValue: () => "#3794FF" };
  const ctx = vm.createContext({
    document: doc, window: win, console, performance,
    setTimeout, clearTimeout, setInterval, clearInterval,
    ResizeObserver: class { observe() {} disconnect() {} },
    getComputedStyle: () => style, devicePixelRatio: 1,
  });
  ctx.globalThis = ctx;

  return {
    ctx,
    byId,
    frames,
    /** 模拟 document 级委派事件（原型普遍用 document.addEventListener 做事件委托） */
    fire(type, target, extra) { for (const fn of docListeners.get(type) || []) fn(fakeEv(target, extra)); },
  };
}

/** 造一个"携带 data-* 的元素"，用来喂给委派事件处理器 */
function probeTarget(data) {
  const el = makeEl("div");
  Object.assign(el.dataset, data);
  return el;
}

/* --------------------------------------------------------------- 探针表 ---- */
// 每个原型：核心交互至少一条路径必须跑通，跑不通就报错（不是"看看就好"）
const PROBES = {
  "webview-chart-panel.html": (dom) => {
    const frames = dom.frames.map(j => JSON.parse(j));
    const first = frames.find(f => f.kind === "result");
    assert(first, "启动未收到 result 帧");
    assert(first.schema.length === 4 && first.rows.length === 12,
      `启动结果集应为 4 列 / 12 行，实为 ${first.schema.length} / ${first.rows.length}`);

    const selX = dom.byId.get("sel-x");
    selX.value = "region";
    const n = dom.frames.length;
    selX.dispatch("change");
    const rt = dom.frames.slice(n).map(j => JSON.parse(j)).find(f => f.kind === "result");
    assert(rt, "改维度后没有回投结果集");
    assert(rt.rows.length === 3, `region 维度应为 3 行，实为 ${rt.rows.length}`);
  },

  "plugin-entry.html": (dom) => {
    const detail = dom.byId.get("detail");
    const listEl = dom.byId.get("list");
    const warnLines = () => listEl.children.filter(c => String(c.className).includes("warn-line"));

    assert(detail.innerHTML.includes("SQL 方言增强"), "启动未渲染详情");
    // 启动态：table-board 是被禁用的、且被项目引用 → 必须有对账行（不能静默）
    assert(warnLines().some(c => String(c.innerHTML).includes("引用") && String(c.innerHTML).includes("禁用")),
      "启动时被禁用的插件没有对账行；children = "
      + (listEl.children.map(c => `[${c.tagName}.${c.className}]${String(c.innerHTML).slice(0, 36)}`).join(" ~ ") || "（空）"));

    // 授予能力 → 「等待授予」卡片应消失
    dom.fire("click", probeTarget({ grant: "webview.export" }));
    assert(!detail.innerHTML.includes("等待授予"),
      "授予 webview.export 后仍有「等待授予」卡片");

    // 重新启用 → 对账行随之消失
    dom.fire("click", probeTarget({ enable: "table-board" }));
    assert(!warnLines().length, "启用后对账行没有消失");

    // 卸载 → 弸层应以「卸载」为题打开，且 OK 后转为「已卸载但项目仍引用」对账行
    dom.fire("click", probeTarget({ uninstall: "table-board" }));
    assert(dom.byId.get("dlg-title").textContent.includes("卸载"),
      "卸载没有打开确认弸层：" + dom.byId.get("dlg-title").textContent);
    dom.byId.get("dlg-ok").dispatch("click");
    assert(warnLines().some(c => String(c.innerHTML).includes("已卸载")),
      "卸载后没有出现「已卸载但项目仍引用」对账行");
  },

  "host-rendered-panel.html": (dom) => {
    // 注意：DOM 桩的 innerHTML 只是字符串，不会长出子元素 —— 所以要查 #list 的 children
    const listEl = () => dom.byId.get("list");
    const hasTableRow = () => listEl().children.some(c => String(c.innerHTML).includes("orders"));
    assert(hasTableRow(), "启动未渲染列表行");

    const cb = dom.byId.get("sim-uninstalled");
    cb.dispatch("change", { target: { checked: true } });
    assert(dom.byId.get("panel-body").innerHTML.includes("未安装插件"),
      "勾「插件被卸载」后没有出现占位");

    cb.dispatch("change", { target: { checked: false } });
    assert(dom.byId.get("panel-body").innerHTML.includes("Top N"), "取消「卸载」后列表没回来");
  },

  "driver-plugin.html": (dom) => {
    const detail = dom.byId.get("detail");
    assert(detail.innerHTML.includes("oracle-jdbc"), "启动未渲染驱动详情");

    dom.fire("click", probeTarget({ nat: "env.inherit", on: "1" }));
    assert(detail.innerHTML.includes("env.inherit"), "native 权限行不见了");

    dom.fire("click", probeTarget({ act: "kill" }));
    assert(dom.byId.get("st-driver").textContent.includes("已退出"), "杀进程后状态栏没变");

    dom.fire("click", probeTarget({ act: "test" }));
  },

  "dashboard-window.html": (dom) => {
    const frames = dom.frames.map(j => JSON.parse(j));
    const begin = frames.find(f => f.kind === "resultBegin");
    assert(begin, "启动没有收到 resultBegin 帧");
    assert(begin.format === "arrow-ipc", `默认投喂方式应为 arrow-ipc，实为 ${begin.format}`);

    const msg = dom.byId.get("rx-progress-text");
    dom.byId.get("btn-slow").dispatch("click");
    dom.byId.get("btn-cancel").dispatch("click");
    assert(msg.textContent.includes("已取消"), `取消后进度文案应为「已取消」，实为「${msg.textContent}」`);
  },
};

function assert(cond, msg) { if (!cond) throw new Error(msg); }

/* --------------------------------------------------------------- 主流程 ---- */
const files = readdirSync(HERE).filter(f => f.endsWith(".html")).sort();
if (!files.length) { console.error("目录里没有 .html 原型"); process.exit(1); }

let bad = 0;
for (const f of files) {
  const html = readFileSync(join(HERE, f), "utf8");
  const open = html.lastIndexOf("<script>");
  const close = html.lastIndexOf("</script>");
  const problems = [];
  let dom = null;

  if (open < 0 || close < 0) problems.push("找不到内联 <script> 块");
  else {
    const src = html.slice(open + "<script>".length, close);

    try { new vm.Script(src, { filename: f }); }
    catch (e) { problems.push("语法错误：" + e.message); }

    if (!problems.length) {
      const errs = [];
      const orig = console.error;
      console.error = (...a) => errs.push("console.error: " + a.join(" "));
      dom = installDom();
      try {
        new vm.Script(src, { filename: f }).runInContext(dom.ctx, { timeout: 4000 });
      } catch (e) {
        problems.push("加载期抛错：" + (e && e.message ? e.message : String(e)));
      } finally {
        console.error = orig;
      }
      problems.push(...errs);
    }

    // 交互探针
    if (!problems.length && dom) {
      const probe = PROBES[f];
      if (!probe) problems.push("没有为它写交互探针（新增原型请在 PROBES 里登记）");
      else {
        try { probe(dom); }
        catch (e) { problems.push("交互探针失败：" + e.message); }
      }
    }
  }

  if (!/lang="zh-CN"/.test(html)) problems.push('缺 lang="zh-CN"');
  if (!/data-theme="dark"/.test(html) || !/data-theme="light"/.test(html)) problems.push("缺明暗两套 token");

  if (problems.length) {
    bad += 1;
    console.log(`✗ ${f}`);
    for (const p of problems) console.log("    " + p);
  } else {
    console.log(`✓ ${f}`);
  }
}

console.log(bad ? `\n${bad} 个原型有问题` : `\n${files.length} 个原型全部通过（语法 + 加载 + 交互探针）`);
process.exit(bad ? 1 : 0);
