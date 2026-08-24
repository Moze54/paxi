# paxi

基于 Tauri 2 + React + TypeScript 的桌面端应用。

## 环境要求

- [Node.js](https://nodejs.org/) 18+
- [pnpm](https://pnpm.io/)
- [Rust](https://www.rust-lang.org/) 及 Tauri 系统依赖（Windows 需安装 MSVC 构建工具 + WebView2）

## 开发

```bash
pnpm install   # 安装前端依赖
pnpm dev       # 启动开发模式（会打开桌面窗口，支持热更新）
```

## 构建打包

```bash
pnpm build     # 打包当前平台的安装包
```

## 项目结构

```
src/                    # React 前端源码（Tauri 加载的 UI）
src-tauri/              # Rust 后端（主逻辑、窗口配置、权限）
  src/lib.rs            # 应用入口与命令定义
  src/main.rs           # 桌面进程入口
  tauri.conf.json       # Tauri 配置（窗口、打包等）
  capabilities/         # 权限能力声明
```

## 说明

- 本项目为纯桌面端应用，UI 由 Tauri 的 WebView 加载，无需单独启动浏览器。
- `pnpm dev` 会由 Tauri 自动拉起 Vite 开发服务器，前端改动即时热更新。
