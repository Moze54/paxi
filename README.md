# paxi 抓包工具

基于 Tauri 2 + React + TypeScript + Rust 自研引擎的桌面端抓包工具，支持抓取本机与手机的 HTTP/HTTPS 流量，内置 AI 分析能力。

## 功能特性

- 🚀 **HTTP/HTTPS 抓包**：Rust 异步中间人代理引擎，高性能抓取
- 🔒 **HTTPS 解密**：自签 CA + 动态域名证书，一键导出根证书
- 📱 **手机抓包**：标准 HTTP 代理，手机连同一 Wi-Fi 即可抓 App 流量
- 🔍 **实时查看**：请求列表实时刷新，方法/状态码彩色标注
- 📋 **详情查看**：请求/响应头与 body，JSON 自动格式化
- 🤖 **AI 分析**：接入 DeepSeek/OpenAI 等兼容 API，一键分析接口用途、参数、加密签名
- 🔎 **过滤搜索**：按 URL / 方法 / 状态码过滤

## 环境要求

- [Node.js](https://nodejs.org/) 18+ 与 [pnpm](https://pnpm.io/)
- [Rust](https://www.rust-lang.org/)（Windows 需安装 MSVC 构建工具 + WebView2）

## 开发

```bash
pnpm install   # 安装依赖
pnpm dev       # 启动开发模式（弹出桌面窗口）
```

## 构建打包

```bash
pnpm build     # 打包当前平台安装包
```

## 使用说明

### 1. 抓本机流量
1. 点击「启动」按钮，代理监听 `8888` 端口
2. 在浏览器/系统设置中将 HTTP 代理指向 `127.0.0.1:8888`
3. 访问网页即可看到抓到的请求

### 2. 抓 HTTPS（需先解密）
1. 打开「设置」→「导出根证书」
2. 按提示安装并信任根证书（Windows 安装到「受信任的根证书颁发机构」）
3. 之后即可解密 HTTPS 流量

### 3. 抓手机 App
1. 手机与电脑连接**同一 Wi-Fi**
2. 查看「设置」中的代理地址（如 `192.168.1.100:8888`）
3. 手机 Wi-Fi 设置中配置手动代理，填入该地址
4. 用手机浏览器访问电脑 IP 下载证书并安装（iOS 需额外在「设置 → 通用 → 关于本机 → 证书信任设置」中开启）
5. 打开 App 即可抓到手机的 HTTPS 流量

### 4. AI 分析
1. 点击任意请求详情的「AI 分析」按钮
2. 在弹窗中配置 API Base URL、API Key、模型名（支持 DeepSeek、OpenAI 等兼容接口）
3. 点击「开始分析」，AI 将解读接口用途、参数含义、加密签名等

## 项目结构

```
src/                          # React 前端
  components/                 # UI 组件
    Toolbar.tsx               # 顶部工具栏（启动/停止/搜索/清空）
    RequestList.tsx           # 请求列表
    RequestDetail.tsx         # 请求详情（概览/请求/响应）
    AiPanel.tsx               # AI 分析面板
    Settings.tsx              # 设置（证书导出/手机连接指引）
  lib/
    ipc.ts                    # Tauri IPC 封装与类型定义
    store.ts                  # Zustand 状态管理
src-tauri/                    # Rust 后端
  src/
    main.rs                   # 桌面进程入口
    lib.rs                    # 应用入口 + Tauri commands
    ca.rs                     # CA 证书管理（自签根证书 + 动态叶子证书）
    proxy.rs                  # HTTP/HTTPS 中间人代理引擎
    models.rs                 # 流量记录数据结构与存储
  tauri.conf.json             # Tauri 配置
  capabilities/               # 权限声明
```

## 技术栈

- **引擎**：Rust（tokio + hyper + rustls），异步 HTTP/HTTPS 中间人代理
- **证书**：rcgen 自签 CA，按域名动态签发叶子证书
- **前端**：React 19 + TypeScript + Vite + Zustand + lucide-react
- **桌面**：Tauri 2

## 后续规划

- [ ] HTTP/2 支持
- [ ] WebSocket 帧解析
- [ ] 断点与请求重放/篡改
- [ ] HAR 导入导出
- [ ] 流量落盘与历史会话
- [ ] 自动识别接口并生成重放脚本
