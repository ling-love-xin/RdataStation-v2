// 右侧详情：基本信息 / 贡献点 / 权限 / 项目引用 / 运行状态。
"use strict";

  /* ================================================================ 渲染：详情 */
  function renderDetail() {
    const detail = $("plugin-detail");
    const p = state.selectedId ? findPlugin(state.selectedId) : null;
    if (!p) {
      detail.innerHTML = '<div class="detail-empty"><div class="big">未选择插件</div><div>从左侧列表或卡片里选一个，查看贡献点与运行状态。</div></div>';
      return;
    }
    const c = p.contributions;
    const total = c.commands + c.panels + c.settings + c.drivers;
    const capRows = p.caps.length
      ? p.caps.map(function (cap) {
          const st = cap.state === "granted" ? "已授予" : cap.state === "pending" ? "待授权" : "未授予";
          return '<div class="perm-row">' +
            '<span class="perm-dot ' + cap.state + '"></span>' +
            '<div class="perm-copy"><b>' + esc(cap.name) + '</b><em class="mono">' + esc(cap.key) + '</em></div>' +
            '<span class="perm-state ' + cap.state + '">' + st + '</span></div>';
        }).join("")
      : '<div class="ref-none">没有声明宿主能力（或清单在解析前就被拒绝）。</div>';
    const refs = p.refs.length
      ? p.refs.map(function (r) {
          return '<div class="ref-warning">' +
            '<div>本项目引用了它贡献的面板：<b>' + esc(r.detail) + '</b>。插件当前' +
            (p.enabled ? '已启用。' : '已停用，面板不会出现。') + '</div>' +
            '<div class="actions">' +
              (p.enabled ? '' : '<button class="btn btn-xs btn-tint" data-toggle="' + esc(p.id) + '">启用插件</button>') +
              '<button class="btn btn-xs btn-ghost" data-remove-ref="' + esc(p.id) + '">从项目移除引用</button>' +
            '</div></div>';
        }).join("")
      : '<div class="ref-none">没有被项目引用。</div>';
    const detailActions = p.status === "error"
      ? '<button class="btn btn-xs btn-tint" data-retry="' + esc(p.id) + '">重试</button>' +
        '<button class="btn btn-xs btn-danger-ghost" data-uninstall="' + esc(p.id) + '">卸载</button>'
      : (p.updateAvailable ? '<button class="btn btn-xs btn-tint" data-upgrade="' + esc(p.id) + '">升级到 ' + esc(p.latest) + '</button>' : '') +
        (p.dev ? '<button class="btn btn-xs btn-ghost" data-rebuild="' + esc(p.id) + '">重构</button>' : '') +
        '<button class="btn btn-xs btn-ghost" data-toggle="' + esc(p.id) + '">' + (p.enabled ? "停用" : "启用") + '</button>';

    detail.innerHTML =
      '<div class="detail-head">' +
        '<div class="detail-title-row">' +
          '<div class="detail-mark ' + kindClass(p) + '">' + esc(p.mark) + '</div>' +
          '<div class="detail-name-wrap">' +
            '<div class="detail-name">' + esc(p.name) + '</div>' +
            '<div class="detail-id mono">' + esc(p.id) + '</div>' +
          '</div>' +
          '<button class="icon-btn" data-close-detail title="收起详情">✕</button>' +
        '</div>' +
        '<div class="detail-badges">' +
          '<span class="status-pill ' + statusClass(p) + '"><span class="dot"></span>' + statusLabel(p) + '</span>' +
          '<span class="chip">' + kindLabel(p) + '</span>' +
          (p.dev ? '<span class="micro-tag dev">DEV</span>' : '') +
          (p.updateAvailable ? '<span class="micro-tag update">可更新 ' + esc(p.latest) + '</span>' : '') +
        '</div>' +
        '<div class="detail-actions">' + detailActions + '<span class="spacer"></span>' +
          '<button class="btn btn-xs btn-ghost" data-detail-more>⋯</button>' +
        '</div>' +
      '</div>' +

      '<section class="detail-section">' +
        '<h3>基本信息<span class="rule"></span></h3>' +
        '<dl class="kv-grid">' +
          '<dt>插件 ID</dt><dd class="mono">' + esc(p.id) + '</dd>' +
          '<dt>类型</dt><dd>' + kindLabel(p) + ' · ' + (p.kind === "driver" ? "驱动轨" : "扩展轨") + '</dd>' +
          '<dt>版本</dt><dd>v' + esc(p.version) + (p.updateAvailable ? ' <span class="muted">→ v' + esc(p.latest) + '</span>' : '') + '</dd>' +
          '<dt>作者</dt><dd>' + esc(p.author) + '</dd>' +
          '<dt>安装</dt><dd>' + esc(p.installed) + ' · ' + esc(p.size) + '</dd>' +
          '<dt>来源</dt><dd class="mono">' + esc(p.source) + '</dd>' +
          '<dt>完整性</dt><dd>' + esc(p.checksum) + '</dd>' +
        '</dl>' +
      '</section>' +

      '<section class="detail-section">' +
        '<h3>贡献点<span class="rule"></span></h3>' +
        '<div class="contrib-grid">' +
          '<div class="contrib-stat' + (c.commands ? "" : " zero") + '"><span>命令</span><b>' + c.commands + '</b></div>' +
          '<div class="contrib-stat' + (c.panels ? "" : " zero") + '"><span>面板</span><b>' + c.panels + '</b></div>' +
          '<div class="contrib-stat' + (c.settings ? "" : " zero") + '"><span>设置</span><b>' + c.settings + '</b></div>' +
          '<div class="contrib-stat' + (c.drivers ? "" : " zero") + '"><span>驱动</span><b>' + c.drivers + '</b></div>' +
        '</div>' +
        '<p class="hint">共 <strong>' + total + '</strong> 个贡献点。' +
          (p.kind === "driver"
            ? '驱动不进入命令/设置注册表，连接能力由 sidecar 描述符提供。'
            : '命令执行、Dock 注册与设置页渲染仍在 P4；当前只展示清单声明。') +
        '</p>' +
      '</section>' +

      '<section class="detail-section">' +
        '<h3>权限声明<span class="rule"></span></h3>' + capRows +
      '</section>' +

      '<section class="detail-section">' +
        '<h3>项目引用<span class="rule"></span></h3>' + refs +
      '</section>' +

      '<section class="detail-section">' +
        '<h3>运行状态<span class="rule"></span></h3>' +
        '<div class="runtime-line"><span class="dot ' + (p.status === "error" ? "error" : p.enabled ? "" : "disabled") + '"></span>' +
          '<span>' + esc(p.runtime.mode) + '</span></div>' +
        (p.status === "error"
          ? '<div class="runtime-meta">' + esc(p.error) + '</div>'
          : '<div class="runtime-meta">PID ' + esc(p.runtime.pid) + ' · 已运行 ' + esc(p.runtime.uptime) + ' · 内存 ' + esc(p.runtime.memory) + '</div>') +
      '</section>' +

      '<section class="detail-section">' +
        '<div class="danger-zone"><span class="muted" style="font-size:10.5px">卸载会删除安装目录，保留 plugin-data。</span>' +
          '<span class="spacer"></span>' +
          '<button class="btn btn-xs btn-danger-ghost" data-uninstall="' + esc(p.id) + '">卸载</button>' +
        '</div>' +
      '</section>';
  }
