// 纯函数：转义 / 状态文案 / 计数 / 筛选 / 贡献点 chips。
"use strict";

  /* ================================================================ 工具函数 */
  function $(id) { return document.getElementById(id); }

  function esc(s) {
    return String(s == null ? "" : s)
      .replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;")
      .replace(/"/g, "&quot;").replace(/'/g, "&#39;");
  }

  function statusLabel(p) {
    if (p.status === "error") return "出错";
    if (!p.enabled) return "已停用";
    if (p.status === "loaded") return "已加载";
    return "已激活";
  }

  function statusClass(p) {
    if (p.status === "error") return "error";
    if (!p.enabled) return "disabled";
    if (p.status === "loaded") return "loaded";
    return "active";
  }

  function kindLabel(p) {
    return p.kind === "driver" ? "驱动" : "界面扩展";
  }

  function kindClass(p) {
    if (p.status === "error") return "error";
    if (p.dev) return "dev";
    return p.kind;
  }

  function counts() {
    const all = state.plugins;
    return {
      all: all.length,
      active: all.filter(function (p) { return p.enabled && !p.status.startsWith("error"); }).length,
      disabled: all.filter(function (p) { return !p.enabled && p.status !== "error"; }).length,
      error: all.filter(function (p) { return p.status === "error"; }).length,
      updates: all.filter(function (p) { return p.updateAvailable; }).length,
      driver: all.filter(function (p) { return p.contributions.drivers > 0 || p.kind === "driver"; }).length,
      panels: all.filter(function (p) { return p.contributions.panels > 0; }).length,
      commands: all.filter(function (p) { return p.contributions.commands > 0; }).length,
      settings: all.filter(function (p) { return p.contributions.settings > 0; }).length
    };
  }

  function visiblePlugins() {
    const q = state.query.toLowerCase();
    let list = state.plugins.filter(function (p) {
      if (state.filter === "active" && !(p.enabled && p.status !== "error")) return false;
      if (state.filter === "disabled" && !(!p.enabled && p.status !== "error")) return false;
      if (state.filter === "error" && p.status !== "error") return false;
      if (state.filter === "updates" && !p.updateAvailable) return false;
      if (state.kind === "driver" && !(p.contributions.drivers > 0 || p.kind === "driver")) return false;
      if (state.kind === "panels" && !(p.contributions.panels > 0)) return false;
      if (state.kind === "commands" && !(p.contributions.commands > 0)) return false;
      if (state.kind === "settings" && !(p.contributions.settings > 0)) return false;
      if (!q) return true;
      return (p.name + " " + p.id + " " + p.description + " " + p.author).toLowerCase().indexOf(q) >= 0;
    });
    if (state.sort === "name") {
      list = list.slice().sort(function (a, b) { return a.name.localeCompare(b.name, "zh-Hans-CN"); });
    } else if (state.sort === "update") {
      list = list.slice().sort(function (a, b) { return Number(b.updateAvailable) - Number(a.updateAvailable); });
    } else if (state.sort === "error") {
      list = list.slice().sort(function (a, b) { return Number(b.status === "error") - Number(a.status === "error"); });
    }
    return list;
  }

  function findPlugin(id) {
    return state.plugins.filter(function (p) { return p.id === id; })[0] || null;
  }

  function chipSet(p) {
    const c = p.contributions;
    const out = [];
    if (c.drivers) out.push('<span class="chip driver">驱动 <b>' + c.drivers + '</b></span>');
    if (c.panels) out.push('<span class="chip panel">面板 <b>' + c.panels + '</b></span>');
    if (c.commands) out.push('<span class="chip cmd">命令 <b>' + c.commands + '</b></span>');
    if (c.settings) out.push('<span class="chip setting">设置 <b>' + c.settings + '</b></span>');
    if (!out.length) out.push('<span class="chip none">无贡献点</span>');
    return out.join("");
  }
