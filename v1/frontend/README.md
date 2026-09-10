# RdataStation 前端架构说明

## 目录结构（遵循企业级规范）

```
src/
├── modules/                    # 业务模块（按功能域划分）
│   ├── connection/            # 连接管理模块
│   ├── database/              # 数据库操作模块
│   ├── project/               # 项目管理模块
│   ├── query/                 # 查询模块
│   └── workbench/             # 工作台模块
├── shared/                     # 共享资源
│   ├── components/            # 通用组件
│   ├── composables/           # 组合式函数
│   ├── config/                # 配置文件
│   ├── constants/             # 常量定义
│   ├── services/              # 服务层
│   ├── stores/                # 状态管理
│   ├── styles/                # 全局样式
│   ├── types/                 # 类型定义
│   └── utils/                 # 工具函数
├── adapters/                   # 数据库适配器（元数据）
├── app/                        # 应用入口
│   ├── App.vue
│   ├── main.ts
│   └── router.ts
└── assets/                     # 静态资源
```

## 模块结构规范

每个模块内部结构：

```
modules/{module-name}/
├── components/       # 模块私有组件
├── composables/      # 模块私有组合式函数
├── services/         # 模块私有服务
├── stores/           # 模块私有状态
├── types/            # 模块私有类型
└── index.ts          # 模块导出
```

## 依赖规则

1. **模块间依赖**：模块只能通过 `shared` 或明确导出的接口通信
2. **禁止循环依赖**：模块A不能依赖模块B，同时模块B依赖模块A
3. **共享层**：`shared` 层不能依赖任何业务模块
4. **适配器层**：`adapters` 只能被 `shared/services` 或模块使用
