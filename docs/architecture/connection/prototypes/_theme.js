/* RdataStation 原型集 · 明暗主题切换（localStorage 记忆，跨页保持） */
(function () {
  var KEY = 'rds-proto-theme';
  var saved = null;
  try { saved = localStorage.getItem(KEY); } catch (e) { /* file:// 某些环境禁用 */ }
  var prefersDark = false;
  try { prefersDark = window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)').matches; } catch (e) {}
  var theme = saved || (prefersDark ? 'dark' : 'light');
  document.documentElement.setAttribute('data-theme', theme);

  window.addEventListener('DOMContentLoaded', function () {
    document.querySelectorAll('[data-theme-toggle]').forEach(function (btn) {
      function sync() {
        var cur = document.documentElement.getAttribute('data-theme');
        btn.textContent = cur === 'dark' ? '☀️ 亮色' : '🌙 暗色';
      }
      btn.addEventListener('click', function () {
        var cur = document.documentElement.getAttribute('data-theme');
        var next = cur === 'dark' ? 'light' : 'dark';
        document.documentElement.setAttribute('data-theme', next);
        try { localStorage.setItem(KEY, next); } catch (e) {}
        sync();
      });
      sync();
    });
  });

  /* 轻量页内 Tab 切换：data-tab-panel 组 */
  window.addEventListener('DOMContentLoaded', function () {
    document.querySelectorAll('[data-tabs]').forEach(function (group) {
      var tabs = group.querySelectorAll('[data-tab]');
      tabs.forEach(function (tab) {
        tab.addEventListener('click', function () {
          tabs.forEach(function (t) { t.classList.remove('on'); });
          tab.classList.add('on');
          var name = tab.getAttribute('data-tab');
          document.querySelectorAll('[data-panel]').forEach(function (p) {
            if (p.getAttribute('data-panel') === name) { p.classList.remove('hidden'); }
            else { p.classList.add('hidden'); }
          });
        });
      });
    });
  });
})();
