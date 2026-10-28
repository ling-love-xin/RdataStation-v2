# RdataStation 主题系统说明

> 本文档是 RdataStation-v2 的主题系统权威说明。基于 gpui-kit 0.8.1 的 ThemeColor 系统。

---

## 一、主题架构

### 1.1 主题层级

```
Theme（主题）
├── colors: ThemeColor（颜色系统，141 个 token）
├── radius: 圆角
├── font_size: 字号基准
└── 其他...
```

### 1.2 明暗模式

- **Light**：浅色主题（白底深字）
- **Dark**：深色主题（黑底浅字）
- **System**：跟随系统自动切换

切换方式：`Ctrl+Shift+L` 或设置页 → 外观 → 主题模式。

---

## 二、颜色 Token 系统

### 2.1 基础色

| Token | 用途 |
|---|---|
| `background` | 主背景色 |
| `foreground` | 主文字色 |
| `border` | 边框色 |
| `muted_foreground` | 辅助文字色 |

### 2.2 语义色

| Token | 用途 |
|---|---|
| `primary` | 品牌色（主按钮、选中态） |
| `success` | 成功（绿色） |
| `warning` | 警告（黄色） |
| `danger` / `error` | 错误（红色） |
| `info` | 信息（蓝色） |

每个语义色都有对应的变体：
- `xxx_hover`：hover 态
- `xxx_active`：按下态
- `xxx_foreground`：文字色

### 2.3 组件色

| Token | 用途 |
|---|---|
| `button` | 按钮背景 |
| `button_hover` | 按钮 hover 背景 |
| `button_active` | 按钮按下背景 |
| `button_foreground` | 按钮文字 |

按钮变体：
- `button_primary` / `button_secondary` / `button_danger` / `button_success` / `button_warning` / `button_info`

### 2.4 布局色

| Token | 用途 |
|---|---|
| `title_bar` | 标题栏背景 |
| `title_bar_border` | 标题栏底边框 |
| `sidebar` | 侧边栏背景 |
| `sidebar_accent` | 侧边栏激活项高亮 |
| `popover` | 弹出层背景 |
| `overlay` | 遮罩层背景 |

### 2.5 列表色

| Token | 用途 |
|---|---|
| `list` | 列表背景 |
| `list_hover` | 列表项 hover 背景 |
| `list_active` | 列表项选中背景 |

### 2.6 图表色

| Token | 用途 |
|---|---|
| `chart_1` ~ `chart_5` | 图表系列色 |
| `chart_bullish` | 上涨（绿色） |
| `chart_bearish` | 下跌（红色） |
| `chart_grid` | 图表网格线 |

---

## 三、使用规范

### 3.1 取色规则

**硬约束**：
- ✅ 必须用 `cx.theme().colors.<token>`
- ❌ 禁止裸 `hex` / `rgb` / `hsla`

### 3.2 语义色使用场景

| 场景 | 用哪个 token |
|---|---|
| 成功提示、操作成功 | `success` |
| 错误提示、操作失败 | `danger` |
| 警告、注意事项 | `warning` |
| 信息提示、普通通知 | `info` |
| 主按钮、选中态 | `primary` |
| 辅助文字、说明 | `muted_foreground` |

### 3.3 组件色使用场景

| 场景 | 用哪个 token |
|---|---|
| 普通按钮 | `button` |
| 主操作按钮 | `button_primary` |
| 危险操作按钮 | `button_danger` |
| 侧边栏背景 | `sidebar` |
| 弹出层背景 | `popover` |

---

## 四、对比度标准

### 4.1 WCAG AA 标准

- 正文文字对比度 ≥ 4.5:1
- 大文字（18px+ 或 14px Semibold）对比度 ≥ 3:1
- 交互元素（按钮、输入框）对比度 ≥ 3:1

### 4.2 检查方法

- `foreground` on `background`：必须 ≥ 4.5:1
- `muted_foreground` on `background`：必须 ≥ 4.5:1（不能太淡）
- `button_foreground` on `button`：必须 ≥ 4.5:1

---

## 五、主题定制

### 5.1 新增 Token

如果需要新的语义色：
1. 在 `ThemeColor` 结构体里加字段
2. 在 light/dark 主题里定义值
3. 在设计规范文档里登记用途

### 5.2 自定义主题

用户可以在设置页切换：
- Light / Dark / System
- 自定义品牌色（未来扩展）

---

## 六、常见误用

| 误用 | 正确做法 |
|---|---|
| 用 `border` 当文字色 | 用 `muted_foreground` |
| 用 `sidebar_accent` 当按钮背景 | 用 `button_primary` |
| 用 `danger` 当普通警告 | 用 `warning` |
| 用 `info` 当成功提示 | 用 `success` |
| 裸 `#ff0000` 写死颜色 | 用 `cx.theme().colors.danger` |
