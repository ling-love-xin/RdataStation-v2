---
scene: git_message
---

name: Gitmoji-Angular 提交规范（融合版）
description: 强制使用 Gitmoji 表情 + Angular 结构化提交信息，美观、规范、GitHub 友好
version: 1.0.0
author: 开发规范

# 提交信息生成规则

commitRules:

# 基础格式：表情 类型(模块): 描述

format: "<emoji> <type>(<scope>): <subject>"

# 简短格式（无模块时）

shortFormat: "<emoji> <type>: <subject>"

# 类型定义（Angular 标准）

types: - label: 新功能
value: feat
emoji: "✨"
desc: 新增功能、页面、组件

    - label: 修复 Bug
      value: fix
      emoji: "🐛"
      desc: 修复问题、逻辑错误

    - label: 文档
      value: docs
      emoji: "📝"
      desc: README、注释、说明文档

    - label: 重构
      value: refactor
      emoji: "♻️"
      desc: 代码重构，无新功能/无Bug修复

    - label: 性能优化
      value: perf
      emoji: "⚡️"
      desc: 提升性能、优化查询、减少耗时

    - label: 样式调整
      value: style
      emoji: "💄"
      desc: 格式化、空格、换行，不影响逻辑

    - label: 测试
      value: test
      emoji: "🧪"
      desc: 单元测试、集成测试

    - label: 构建/依赖
      value: build
      emoji: "📦"
      desc: 构建脚本、依赖更新

    - label: 杂项/配置
      value: chore
      emoji: "🔧"
      desc: 配置修改、工具调整

# 常用示例（AI 自动参考）

examples:

- "✨ feat(layout): 实现页面拖拽排序功能"
- "🐛 fix(sqlite): 修复百万级数据查询超时问题"
- "📝 docs: 补充元数据库设计与注释说明"
- "♻️ refactor(metadata): 统一表结构与字段命名"
- "⚡️ perf(sql): 优化索引提升查询速度"
- "🔧 chore: 初始化项目开发规范"

# AI 指令（核心生效逻辑）

prompt: |
你必须严格按照【Gitmoji + Angular 融合规范】生成提交信息：

1. 格式固定：【表情 类型(模块): 描述】
2. 表情必须与类型一一对应
3. 类型只能使用：feat、fix、docs、refactor、perf、style、test、build、chore
4. 描述简洁清晰，使用中文，不超过50字
5. 禁止空提交、禁止无意义提交
