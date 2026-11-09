// 启停、升级、重试、移除项目引用、toast。
"use strict";

  /* ================================================================ 交互 */
  function selectPlugin(id) {
    state.selectedId = id;
    render();
    const grid = $("plugin-grid");
    const card = Array.prototype.slice.call(grid.children).filter(function (el) {
      return el.dataset && el.dataset.select === id;
    })[0];
    if (card && typeof card.scrollIntoView === "function") {
      card.scrollIntoView({ block: "nearest" });
    }
  }

  function togglePlugin(id) {
    const p = findPlugin(id);
    if (!p || p.status === "error") return;
    p.enabled = !p.enabled;
    p.status = p.enabled ? (p.kind === "driver" ? "active" : "loaded") : "disabled";
    p.runtime = p.enabled
      ? (p.kind === "driver"
          ? { mode: "sidecar · 已启动", pid: "5" + (100 + Math.floor(Math.random() * 800)), uptime: "刚刚", memory: "—", state: "active" }
          : { mode: "注册表已加载 · 执行接线未落地", pid: "—", uptime: "—", memory: "—", state: "loaded" })
      : { mode: "已停用 · 不占进程", pid: "—", uptime: "—", memory: "—", state: "disabled" };
    showToast(p.name + (p.enabled ? " 已启用" : " 已停用") + "（原型内状态，不落盘）");
    render();
  }

  function upgradePlugin(id) {
    const p = findPlugin(id);
    if (!p || !p.updateAvailable) return;
    p.version = p.latest;
    p.updateAvailable = false;
    showToast(p.name + " 已升级到 v" + p.version + "（原型内状态）");
    render();
  }

  function retryPlugin(id) {
    const p = findPlugin(id);
    if (!p) return;
    p.status = "active";
    p.enabled = true;
    p.error = null;
    p.runtime = { mode: "注册表已加载 · 执行接线未落地", pid: "—", uptime: "—", memory: "—", state: "loaded" };
    showToast(p.name + " 已重新加载（原型内状态）");
    render();
  }

  function removeReference(id) {
    const p = findPlugin(id);
    if (!p) return;
    p.refs = [];
    showToast("已从项目移除对「" + p.name + "」的引用（原型内状态）");
    render();
  }

  function showToast(text) {
    const toast = $("toast");
    toast.textContent = text;
    toast.classList.add("show");
    if (state.toastTimer) clearTimeout(state.toastTimer);
    state.toastTimer = setTimeout(function () {
      toast.classList.remove("show");
    }, 2200);
  }
