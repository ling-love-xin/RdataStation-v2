// 安装 / 卸载确认弹层。
"use strict";

  /* ---------------------------------------------------------------- 弹层 */
  function openDialog(kind, id) {
    state.dialog = { kind: kind, id: id || null };
    const dlg = $("dialog");
    const title = $("dlg-title");
    const body = $("dlg-body");
    const foot = $("dlg-foot");
    dlg.classList.remove("hidden");
    dlg.setAttribute("aria-hidden", "false");

    if (kind === "install") {
      title.textContent = "安装插件";
      body.innerHTML =
        '<div class="drop"><b>把插件包拖到这里</b><span>支持 .zip / .tar.gz，或只套一层插件目录的包</span></div>' +
        '<div class="mono-note">当前安装链已实现：清单校验 / 入口存在性 / 可选 checksums.json 的 SHA-256。' +
        '签名（P5）与 .rdsx 后缀尚未实现，本弹层只演示入口。</div>';
      foot.innerHTML = '<button class="btn" data-dialog-close>取消</button>' +
        '<button class="btn btn-primary" data-dialog-confirm>选择 .zip</button>';
      return;
    }

    const p = findPlugin(id);
    if (!p) return;
    title.textContent = "卸载 " + p.name;
    body.innerHTML =
      '<div>将从 <span class="mono">' + esc(p.source) + '</span> 删除安装目录，并清掉插件登记。</div>' +
      '<div class="mono-note">保留 <span class="mono">plugin-data/' + esc(p.id) + '/</span>；' +
      '若其它插件声明了对它的依赖，卸载会被拒绝并提示依赖链。</div>' +
      (p.refs.length ? '<div class="ref-warning" style="margin-top:10px">本项目仍引用它贡献的面板；卸载后会留下「已卸载但项目仍引用」的对账项。</div>' : '');
    foot.innerHTML = '<button class="btn" data-dialog-close>取消</button>' +
      '<button class="btn btn-danger-ghost" data-dialog-confirm>确认卸载</button>';
  }

  function closeDialog() {
    state.dialog = null;
    const dlg = $("dialog");
    dlg.classList.add("hidden");
    dlg.setAttribute("aria-hidden", "true");
  }

  function confirmDialog() {
    if (!state.dialog) return closeDialog();
    const d = state.dialog;
    if (d.kind === "install") {
      closeDialog();
      showToast("原型：已选择插件包；真实安装请走 plugin-entry.html 的安装链");
      return;
    }
    if (d.kind === "uninstall") {
      const p = findPlugin(d.id);
      const name = p ? p.name : d.id;
      state.plugins = state.plugins.filter(function (x) { return x.id !== d.id; });
      if (state.selectedId === d.id) {
        state.selectedId = state.plugins.length ? state.plugins[0].id : null;
      }
      closeDialog();
      showToast("已卸载「" + name + "」（原型内状态，保留 plugin-data）");
      render();
    }
  }
