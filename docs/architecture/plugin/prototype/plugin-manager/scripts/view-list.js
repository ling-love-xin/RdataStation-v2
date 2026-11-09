// 统计行、状态段、贡献点 chips、卡片网格。
"use strict";

  /* ================================================================ 渲染：统计 / 筛选 / 卡片 */
  function renderStats() {
    const c = counts();
    $("stats").innerHTML =
      '<span><b>' + c.all + '</b> 已安装</span>' +
      '<span class="sep"></span>' +
      '<span class="ok"><b>' + c.active + '</b> 已启用</span>' +
      '<span><b>' + c.disabled + '</b> 已停用</span>' +
      '<span class="danger"><b>' + c.error + '</b> 出错</span>' +
      '<span class="sep"></span>' +
      '<span class="info"><b>' + c.updates + '</b> 可更新</span>' +
      '<span class="sep"></span>' +
      '<span>驱动 <b>' + c.driver + '</b></span>' +
      '<span>界面扩展 <b>' + (c.all - state.plugins.filter(function (p) { return p.kind === "driver"; }).length) + '</b></span>';
  }

  function renderTabs() {
    const c = counts();
    const rows = [
      { key: "all", label: "全部", n: c.all },
      { key: "active", label: "已启用", n: c.active },
      { key: "disabled", label: "已停用", n: c.disabled },
      { key: "error", label: "出错", n: c.error },
      { key: "updates", label: "可更新", n: c.updates }
    ];
    $("tabs").innerHTML = rows.map(function (r) {
      return '<button class="seg' + (state.filter === r.key ? " on" : "") + '" data-filter="' + r.key + '">' +
        r.label + ' <span class="count">' + r.n + '</span></button>';
    }).join("");
  }

  function renderKinds() {
    const c = counts();
    const rows = [
      { key: "all", label: "全部", n: c.all },
      { key: "driver", label: "驱动", n: c.driver },
      { key: "panels", label: "面板", n: c.panels },
      { key: "commands", label: "命令", n: c.commands },
      { key: "settings", label: "设置", n: c.settings }
    ];
    $("kinds").innerHTML = rows.map(function (r) {
      return '<button class="kind-chip' + (state.kind === r.key ? " on" : "") + '" data-kind="' + r.key + '">' +
        r.label + ' <span class="n">' + r.n + '</span></button>';
    }).join("");
  }

  function cardActions(p) {
    if (p.status === "error") {
      return '<button class="btn btn-xs btn-tint" data-retry="' + esc(p.id) + '">重试</button>' +
        '<button class="btn btn-xs btn-danger-ghost" data-uninstall="' + esc(p.id) + '">卸载</button>';
    }
    const btns = [];
    if (p.updateAvailable) {
      btns.push('<button class="btn btn-xs btn-tint" data-upgrade="' + esc(p.id) + '">升级</button>');
    }
    if (p.dev) {
      btns.push('<button class="btn btn-xs btn-ghost" data-rebuild="' + esc(p.id) + '">重构</button>');
    }
    btns.push('<button class="btn btn-xs btn-ghost" data-toggle="' + esc(p.id) + '">' +
      (p.enabled ? "停用" : "启用") + '</button>');
    btns.push('<button class="btn btn-xs btn-ghost" data-detail="' + esc(p.id) + '">详情</button>');
    return btns.join("");
  }

  function renderGrid() {
    const list = visiblePlugins();
    if (!list.some(function (p) { return p.id === state.selectedId; })) {
      state.selectedId = list.length ? list[0].id : null;
    }
    const grid = $("plugin-grid");
    grid.innerHTML = "";
    if (!list.length) {
      const empty = document.createElement("div");
      empty.className = "empty-state";
      empty.innerHTML = '<div class="big">没有匹配的插件</div>' +
        '<div>换个关键词，或把筛选切回「全部」。</div>' +
        '<div class="hint">当前条件：' + esc(filterHint()) + '</div>';
      grid.appendChild(empty);
      return;
    }
    list.forEach(function (p) {
      const card = document.createElement("article");
      card.className = "plugin-card" + (state.selectedId === p.id ? " selected" : "") +
        (p.status === "error" ? " error" : "") +
        (!p.enabled && p.status !== "error" ? " disabled" : "") +
        (p.dev ? " dev" : "");
      card.dataset.select = p.id;
      const extraTags = (p.dev ? '<span class="micro-tag dev">DEV</span>' : '') +
        (p.updateAvailable ? '<span class="micro-tag update">可更新 ' + esc(p.latest) + '</span>' : '');
      card.innerHTML =
        '<div class="card-top">' +
          '<div class="plugin-mark ' + kindClass(p) + '">' + esc(p.mark) + '</div>' +
          '<div class="card-heading">' +
            '<div class="card-name-line">' +
              '<span class="card-name">' + esc(p.name) + '</span>' +
              '<span class="ver">v' + esc(p.version) + '</span>' +
              extraTags +
            '</div>' +
            '<div class="card-id"><span class="mono">' + esc(p.id) + '</span> · ' + kindLabel(p) + '</div>' +
          '</div>' +
          '<span class="status-pill ' + statusClass(p) + '"><span class="dot"></span>' + statusLabel(p) + '</span>' +
        '</div>' +
        '<p class="card-desc' + (p.status === "error" ? " error-text" : "") + '">' +
          esc(p.status === "error" ? p.error : p.description) + '</p>' +
        '<div class="card-chips">' + chipSet(p) + '</div>' +
        '<div class="card-foot">' +
          '<span class="card-meta">' + esc(p.author) + ' · ' + esc(p.installed) + ' 安装</span>' +
          '<div class="card-actions">' + cardActions(p) + '</div>' +
        '</div>';
      grid.appendChild(card);
    });
  }

  function filterHint() {
    const names = { all: "全部", active: "已启用", disabled: "已停用", error: "出错", updates: "可更新" };
    const kinds = { all: "全部", driver: "驱动", panels: "面板", commands: "命令", settings: "设置" };
    return "状态=" + names[state.filter] + " / 贡献点=" + kinds[state.kind] +
      (state.query ? " / 搜索「" + state.query + "」" : "");
  }
