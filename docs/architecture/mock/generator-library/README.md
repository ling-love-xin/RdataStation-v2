# 生成器库（Generator Library）

列编辑弹窗左栏的生成器浏览区——搜索、双视角分组、自定义生成器。

## 文件

- `mock-generator-filter-interaction.md` —— 筛选交互设计记录
- `mock-generator-filter-prototype.html` —— 筛选原型
- `generator-picker-prototype.html` —— 生成器选择器原型

## 当前结构

```
搜索框（顶）
TabBar：业务视角 | 技术视角
分组列表（可折叠）：
  业务视角：按 GeneratorCategory 分组
  技术视角：按「产出类型」分组（产出整数 / 产出变长文本...）
```
