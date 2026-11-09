// 左 Dock 快速列表与 render 总入口。
"use strict";

  /* ================================================================ 渲染：左 Dock */
  function renderDock() {
    const list = state.plugins;
    $("dock-list").innerHTML = list.map(function (p) {
      const st = statusClass(p);
      return '<button class="dock-item' + (state.selectedId === p.id ? " selected" : "") + '" data-select="' + esc(p.id) + '">' +
        '<span class="dock-mark ' + kindClass(p) + '">' + esc(p.mark) + '</span>' +
        '<span class="dock-copy"><b>' + esc(p.name) + '</b><em>v' + esc(p.version) + ' · ' +
          (p.updateAvailable ? '<span style="color:var(--warn)">可更新</span>' : statusLabel(p)) + '</em></span>' +
        '<span class="status-pill ' + st + '"><span class="dot"></span></span>' +
      '</button>';
    }).join("");
    $("dock-reload").textContent = state.reloadPending ? state.reloadPending + " 待重载" : "已重载";
  }

  function render() {
    renderStats();
    renderTabs();
    renderKinds();
    renderGrid();
    renderDetail();
    renderDock();
  }
