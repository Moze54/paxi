# paxi 抓包工具

基于 Tauri 2 + React + TypeScript + Rust 自研引擎的桌面端抓包工具，支持抓取本机与手机的 HTTP/HTTPS/WebSocket 流量，内置 AI 分析能力。

## 功能特性

- 🚀 **HTTP/HTTPS 抓包**：Rust 异步中间人代理引擎，高性能抓取
- 🔒 **HTTPS 解密**：自签 CA + 动态域名证书，一键导出根证书
- 📱 **手机抓包（扫码即连）**：二维码直达内置证书门户页，自动识别 iOS/Android 分发证书与配置指引，修复了手机访问代理 IP 的回环问题
- ⚡ **WebSocket 帧解析**：双向转发逐帧记录（方向/opcode/payload），详情页时间线查看
- 🔐 **HTTPS 中间人解密**：动态域名叶子证书 + ALPN http/1.1（兼容微信等强制 ALPN 客户端）+ IP 直连证书（微信 mars 等框架）
- 🚇 **TLS 直通**：SSL Pinning / 银行类 App 域名加入直通列表，隧道转发不做 MITM，其余域名照常解密
- 🩺 **失败可见化**：TLS 握手失败 / 证书未信任以记录形式出现在列表，附处理建议
- 🎛️ **规则引擎**：域名/路径/方法通配匹配 × 动作（Mock 响应 / 重定向 / 请求延迟 / 响应延迟 / 拦截 / **断点调试** / **弱网模拟**），优先级排序、命中统计、立即生效
- 🛑 **断点调试**：命中规则挂起请求，弹窗查看/修改后放行或拦截（403），多断点排队、超时自动放行
- 📶 **弱网模拟**：带宽限速（KB/s）+ 首字节延迟 + 丢包率（随机截断响应）
- 📱 **来源应用识别（Windows）**：本机流量按 TCP 连接归属到发起进程（进程名），手机流量归为设备 IP；列表可按「应用」筛选（全部应用 / 指定进程 / 📱 设备 IP）
- 📊 **统计分析**：请求量/成功失败/状态码/方法/协议分布、域名 TOP、耗时、24h 时间线
- 🌐 **上游代理链**：配置二级 HTTP 代理（含基础认证），转发与重放统一经上游；设置页持久化
- ⭐ **请求标记**：右键收藏星标 + 六色标注 + "仅看收藏"过滤（localStorage）
- 🖥️ **CLI 抓包模式**：`paxi-cli [-p PORT]` 无 GUI 运行同一引擎，周期打印新记录，Ctrl+C 优雅停止
- 📦 **HAR 导入/导出**：HAR 1.2 双向，可导入他人的抓包离线分析
- 🤖 **AI 分析（敏感信息脱敏）**：发送前自动打码 Authorization/Cookie/token/sign 等，可关闭
- 🔄 **请求重放**：任意请求编辑后重发（方法/URL/头/体），与源响应行级 diff 对比，REPLAY 徽标标识
- 📋 **复制为 cURL / 代码生成**：cURL（bash/Windows）、fetch、axios、Python requests
- 🖱️ **右键菜单**：复制 URL / cURL / 生成代码 / 重放 / AI 分析
- 📦 **HAR 导出**：HAR 1.2 格式导出全部记录（可被 Chrome DevTools 导入）
- 💾 **SQLite 落盘**：流量与规则持久化存储，大 body 落文件，重启不丢数据
- 🔄 **事件驱动**：后端事件批量推送（100ms 合并窗口）替代轮询，列表实时刷新
- 📜 **虚拟滚动**：万级记录 60fps 流畅滚动
- 🌳 **JSON 树视图**：响应/请求体可折叠树形展示，Query 参数表格，头部表格
- 🌓 **亮/暗主题**：跟随系统 + 手动切换，持久化
- ⌨️ **快捷键**：Ctrl+F 搜索、↑↓ 列表导航、Esc 关闭弹窗
- 🔍 **实时查看**：方法/状态码彩色标注、大小/耗时/来源 IP 一目了然
- 🤖 **AI 分析**：接入 DeepSeek/OpenAI 等兼容 API，一键分析接口用途、参数、加密签名
- 🖥️ **多平台系统代理**：Windows（注册表）与 macOS（networksetup）自动设置/恢复

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

### 3. 抓手机 App（推荐扫码方式）
1. 点击工具栏「连接手机」按钮
2. 手机与电脑连接**同一 Wi-Fi**，用相机扫描弹窗中的二维码，直达证书安装门户页
3. 按门户页指引：配置 Wi-Fi 代理（地址见弹窗）→ 下载安装证书（iOS 需在「证书信任设置」开启完全信任）
4. 打开 App/网页即可看到手机的 HTTPS 流量；工具栏会实时显示已连接的设备数

也可手动操作：手机浏览器直接访问 `http://电脑IP:8888` 即可打开门户页。

### 4. AI 分析
1. 点击任意请求详情的「AI 分析」按钮
2. 在弹窗中配置 API Base URL、API Key、模型名（支持 DeepSeek、OpenAI 等兼容接口）
3. 点击「开始分析」，AI 将解读接口用途、参数含义、加密签名等

```
src/                          # React 前端
```
（结构见上：Toolbar / RequestList / RequestDetail / ConnectPhone /
RulesPanel / BreakpointPanel / StatsPanel / ReplayPanel / CodegenDialog /
ContextMenu / AiPanel / Settings）

### 5. CLI 抓包

```bash
cargo run --bin paxi-cli -- -p 8888            # 指定端口
cargo run --bin paxi-cli -- -d ./data -i 1000  # 指定数据目录与轮询间隔
# Ctrl+C 停止并自动恢复系统代理
```

## 项目结构

```
src/                          # React 前端
  components/                 # UI 组件
    Toolbar.tsx               # 顶部工具栏（启动/搜索/筛选/连接手机/设备数）
    RequestList.tsx           # 请求列表（@tanstack/react-virtual 虚拟滚动）
    RequestDetail.tsx         # 请求详情（概览/请求/响应/WS 帧 Tab + JSON 树）
    ConnectPhone.tsx          # 扫码连接手机弹窗（二维码 + 分步指引）
    RulesPanel.tsx            # 规则管理面板（十种动作含断点/弱网）
    BreakpointPanel.tsx       # 断点调试面板（挂起请求编辑/放行/拦截）
    StatsPanel.tsx            # 流量统计面板（CSS 条形图/时间线）
    ReplayPanel.tsx           # 请求重放编辑器（编辑/结果/diff 三视图）
    CodegenDialog.tsx         # 代码生成弹窗（cURL/fetch/axios/Python）
    ContextMenu.tsx           # 列表右键菜单
    AiPanel.tsx               # AI 分析面板
    Settings.tsx              # 设置（证书导出 / TLS 直通域名）
  lib/
    ipc.ts                    # Tauri IPC 封装、事件订阅与类型定义
    store.ts                  # Zustand 状态管理（事件驱动增量更新）
    filters.ts                # 列表过滤逻辑
    codegen.ts                # cURL/代码生成
    diff.ts                   # 行级 diff（LCS）
    redact.ts                 # AI 脱敏（敏感头/字段递归打码）
    __tests__/                # vitest 前端测试（codegen/diff/filters/redact）
src-tauri/                    # Rust 后端
  src/
    main.rs                   # 桌面进程入口
    lib.rs                    # 应用入口 + Tauri commands
    ca.rs                     # CA 证书管理（自签根证书 + 动态叶子证书，域名 + IP SAN）
    proxy.rs                  # HTTP/HTTPS/WebSocket 中间人代理引擎（ALPN / TLS 直通）
    ws.rs                     # WebSocket 帧转发与逐帧记录
    portal.rs                 # 内置门户（证书下载页 + iOS 描述文件生成）
    rules.rs                  # 规则引擎（匹配器 × 动作 + 命中统计 + SQLite 持久化）
    har.rs                    # HAR 1.2 导出 + 导入
    stats.rs                  # 流量统计聚合
    process.rs                # 来源进程识别（Windows TCP 表 + 进程枚举）
    events.rs                 # 事件批量推送（100ms 合并窗口）
    clients.rs                # 客户端设备感知
    models.rs                 # 流量记录数据结构
    storage/                  # 存储层（trait + SQLite 落盘实现）
    system_proxy.rs           # 系统代理设置（Windows 注册表 / macOS networksetup）
  tests/
    engine_test.rs            # Rust 集成测试（e2e 代理转发 / 规则 / CA / SQLite 迁移 / 重放）
  run-tests.ps1               # 测试运行脚本（处理 Windows 测试 exe 缺 manifest 的问题）
  tauri.conf.json             # Tauri 配置
  capabilities/               # 权限声明
```

## 测试

```bash
# 前端单测（vitest）：codegen / diff / filters —— pnpm test

# Rust 集成测试（须用脚本运行，见下）：真实 socket e2e —— 代理转发、
# 规则 Mock/Redirect/Abort、CA 域名+IP 证书、SQLite 读写与旧库迁移、请求重放
cd src-tauri && powershell -File run-tests.ps1
```

> **Windows 注意**：rustc 生成的测试 exe 没有 manifest，而 tauri 链导入
> `comctl32!TaskDialogIndirect`（仅 Common-Controls v6 存在），直接 `cargo test`
> 会在启动时崩溃（`STATUS_ENTRYPOINT_NOT_FOUND 0xC0000139`）。`run-tests.ps1`
> 会先编译测试并为每个测试 exe 放置同名外置 `.manifest` 再运行，绕开此问题。

## 技术栈

- **引擎**：Rust（tokio + hyper + rustls），异步 HTTP/HTTPS 中间人代理
- **证书**：rcgen 自签 CA，按域名动态签发叶子证书
- **前端**：React 19 + TypeScript + Vite + Zustand + lucide-react
- **桌面**：Tauri 2

## 后续规划

完整的产品与技术规划见 [docs/ROADMAP.md](docs/ROADMAP.md)，按里程碑推进：

- **M1 地基**（P0）：扫码连手机 + 内置证书门户页、WebSocket 帧解析、SQLite 落盘 + 事件推送、三栏 UI 重构（Tab 详情 / JSON 树 / 虚拟滚动 / 暗色主题 / i18n）✅
- **M2 规则与调试**（P1）：规则引擎（Mock / 重定向 / 改写 / Hosts / 延迟 / 拦截）、断点篡改、请求重放与 Composer、弱网模拟、HAR / cURL / 代码生成、AI 面板化 + 敏感信息脱敏 ✅
- **M3 深度**（P2）：HTTP/2 ✅、统计分析面板、上游代理链、请求 Diff
- **M4 生态**（P2）：脚本系统、手机远程查看的 Web 控制台、CLI 模式、QUIC / TUN（远期）
