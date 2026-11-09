// 文档级事件委托、搜索、排序、主题切换、Esc。
"use strict";

  /* ---------------------------------------------------------------- 事件 */
  function closestWith(target, selector) {
    if (!target || typeof target.closest !== "function") return null;
    return target.closest(selector);
  }

  document.addEventListener("click", function (e) {
    const t = e.target;
    const action = closestWith(t, "[data-toggle],[data-upgrade],[data-retry],[data-uninstall],[data-rebuild],[data-detail],[data-remove-ref],[data-select],[data-install-open],[data-import-dir],[data-refresh],[data-page-more],[data-reload],[data-reload-later],[data-dialog-close],[data-dialog-confirm],[data-filter],[data-kind],[data-close-detail],[data-detail-more]");
    if (!action) return;
    const d = action.dataset;

    if (d.toggle) { togglePlugin(d.toggle); e.stopPropagation(); return; }
    if (d.upgrade) { upgradePlugin(d.upgrade); e.stopPropagation(); return; }
    if (d.retry) { retryPlugin(d.retry); e.stopPropagation(); return; }
    if (d.rebuild) { showToast("已请求重构开发扩展（原型内状态）"); e.stopPropagation(); return; }
    if (d.uninstall) { openDialog("uninstall", d.uninstall); e.stopPropagation(); return; }
    if (d.removeRef) { removeReference(d.removeRef); e.stopPropagation(); return; }
    if (d.detail) { selectPlugin(d.detail); e.stopPropagation(); return; }
    if (d.select) { selectPlugin(d.select); e.stopPropagation(); return; }
    if (d.installOpen) { openDialog("install"); e.stopPropagation(); return; }
    if (d.importDir) {
      openDialog("install");
      showToast("从目录导入：选择包含 plugin.toml 的目录（原型内状态）");
      e.stopPropagation(); return;
    }
    if (d.refresh) { showToast("已刷新插件列表（原型内状态）"); e.stopPropagation(); return; }
    if (d.pageMore) { showToast("更多操作：导出诊断、打开插件目录、查看日志（原型占位）"); e.stopPropagation(); return; }
    if (d.reload) {
      state.reloadPending = 0;
      $("reload-banner").classList.add("hidden");
      showToast("已重载 2 个插件（原型内状态）");
      renderDock(); e.stopPropagation(); return;
    }
    if (d.reloadLater) {
      $("reload-banner").classList.add("hidden");
      showToast("已收起重载提示；插件文件变化仍会保留到下次重载");
      e.stopPropagation(); return;
    }
    if (d.dialogClose) { closeDialog(); e.stopPropagation(); return; }
    if (d.dialogConfirm) { confirmDialog(); e.stopPropagation(); return; }
    if (d.filter) { state.filter = d.filter; render(); e.stopPropagation(); return; }
    if (d.kind) { state.kind = d.kind; render(); e.stopPropagation(); return; }
    if (d.closeDetail) { state.selectedId = null; render(); e.stopPropagation(); return; }
    if (d.detailMore) { showToast("更多：复制插件 ID、打开安装目录、查看日志（原型占位）"); e.stopPropagation(); return; }
  });

  $("search").addEventListener("input", function (e) {
    state.query = String(e.target.value || "").trim();
    render();
  });

  $("sort").addEventListener("change", function (e) {
    state.sort = e.target.value;
    render();
  });

  $("theme-toggle").addEventListener("click", function () {
    state.theme = state.theme === "dark" ? "light" : "dark";
    document.documentElement.dataset.theme = state.theme;
    $("theme-toggle").textContent = state.theme === "dark" ? "☀ 亮色" : "☾ 暗色";
  });

  document.addEventListener("keydown", function (e) {
    if (e.key === "Escape") closeDialog();
  });
