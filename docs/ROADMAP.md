# paxi 专业抓包工具 · 完整产品与技术规划

> 版本：v1.0 · 状态：已评审基线
> 定位：面向开发者的一站式 HTTP(S)/WebSocket 调试代理 —— 取 Charles 的易用、mitmproxy 的能力、Whistle 的规则、Reqable 的现代体验，全部装进一个轻量桌面应用。

---

## 1. 对标与定位

| 能力维度 | Charles | Fiddler | mitmproxy | Proxyman | Whistle | Reqable | **paxi 目标** |
|---|---|---|---|---|---|---|---|
| HTTPS 解密 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅（已有） |
| 手机连接引导 | 弱 | 弱 | 无 | 强 | 弱 | 中 | **最强：扫码 + 门户页 + 一键装证书** |
| HTTP/2 | ✅ | 部分 | ✅ | ✅ | ✅ | ✅ | M3 |
| WebSocket 帧 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | M1 |
| 规则/Mock | Map Local | AutoResponder | 脚本 | Breakpoint/Tool | **最强规则系统** | 中 | M2 对齐 Whistle 核心子集 |
| 断点篡改 | ✅ | ✅ | 脚本 | ✅ | ✅ | ✅ | M2 |
| 弱网模拟 | Throttle | 自定义 | 脚本 | Network Link | ✅ | ✅ | M2 |
| 重放/Composer | Compose | Composer | ❌ | ✅ | ❌ | ✅ | M2 |
| HAR 导入导出 | ✅ | ✅ | ✅ | ✅ | ❌ | ✅ | M2 |
| AI 能力 | ❌ | ❌ | ❌ | ❌ | ❌ | 实验 | **差异化：已内置，持续加深** |
| 脚本扩展 | ❌ | FiddlerScript | Python JS | ❌ | JS | ❌ | M4 |
| 跨平台 | Win/Mac/Linux | Win/Mac/Linux | CLI 为主 | macOS only | Node 全平台 | 全平台 | Win/Mac/Linux（Tauri 天然支持） |

**差异化主张**（按优先级）：
1. **连接零门槛**——手机扫码 30 秒内完成"配代理 + 装证书"全套流程，业界目前没有一个工具真正做到。
2. **规则即 UI**——Whistle 的规则能力 + 现代可视化编辑器，不用背语法。
3. **AI 原生**——逆向分析、规则生成、接口文档生成，AI 是一等公民而非外挂弹窗。

---

## 2. 现状盘点（v0.1）

### 2.1 已具备
- Rust 异步 MITM 引擎：CONNECT 隧道 + 动态叶子证书解密 HTTPS（hyper + rustls + rcgen）
- gzip/deflate/brotli 自动解压，文本/二进制粗分
- 基础列表 + 详情 UI、方法/状态/协议筛选、文本搜索
- CA 导出（仅文件导出到桌面）
- Windows 系统代理自动设置/恢复（注册表 + WinINet 通知）
- AI 分析面板（OpenAI 兼容 API）

### 2.2 关键缺陷（按严重度排序）

| # | 缺陷 | 影响 |
|---|---|---|
| D1 | **手机证书下载页不存在**：README 指引手机访问 `http://电脑IP:8888`，实际该请求会被代理原样转发，形成自引用回环，无法下载证书 | 手机抓 HTTPS 基本走不通，核心卖点断裂 |
| D2 | 2s 全量轮询 + 1000 条内存环缓 | 流量稍大 UI 卡顿；重启全部丢失 |
| D3 | 无 WebSocket 帧解析（仅 is_websocket 标记） | 调 App 长连接不可用 |
| D4 | 仅 HTTP/1（服务端 `http1::Builder`，客户端 `enable_http1`） | 大量 App/站点走 H2 时降级甚至失败 |
| D5 | body 512KB 截断、二进制仅显示"[二进制内容]"占位 | 看不了大响应、图片、ProtoBuf |
| D6 | 系统代理仅 Windows | macOS/Linux 用户需手动配 |
| D7 | AI 分析原文发送 Authorization/Cookie 等敏感头 | 隐私泄露风险 |
| D8 | CA 私钥明文 PEM 落盘且无权限收紧 | 安全隐患 |
| D9 | 无规则/断点/重放/Mock/导出/统计 | 距"专业工具"的核心差距 |
| D10 | 无虚拟滚动、无主题、无快捷键、无国际化 | 体验粗糙 |

---

## 3. 设计原则（开发者即用户）

1. **30 秒上手**：从"我想抓个包"到"看到第一条请求"，任何平台（尤其手机）不超过 30 秒、不超过 3 次点击。
2. **流量不丢**：抓到的就是证据。落盘持久化、会话可保存/恢复、崩溃后重开还在。
3. **列表快过眼睛**：10 万条记录 60fps 滚动，事件驱动实时刷新，过滤零延迟。
4. **一切皆可改**：任何请求可以拦截、编辑、重放；任何响应可以 Mock、映射、延迟。
5. **复杂度渐进**：默认界面极简；高级能力（规则、脚本、断点）按需展开，不吓退新手。
6. **键盘优先**：开发者双手不离键盘——`Ctrl+F` 搜索、`Space` 预览、`B` 断点、`R` 重放、`⌘K` 命令面板。
7. **隐私自觉**：敏感头默认脱敏展示，AI 分析前强制脱敏确认，CA 私钥加密存储。

---

## 4. 用户旅程（重点：手机连接）

### 现状之旅（痛苦）
> 打开设置 → 肉眼抄 IP → 手机 Wi-Fi 高级设置 → 手动填代理 → 打Slots浏览器访问 IP（失败，D1）→ 百度搜证书怎么装 → 放弃

### 目标之旅（30 秒）
```
电脑端                          手机端
├─ 点击「连接手机」按钮
│   └─ 弹出大二维码 + 地址卡
│       （二维码内容 = http://IP:PORT/portal）
│                                ├─ 相机扫码 → 直接打开门户页
│                                ├─ 页面自动识别 UA：
│                                │    iOS → 一键安装描述文件(.mobileconfig)
│                                │    Android → 下载 .crt
│                                ├─ 页面显示当前代理地址（可一键复制）
│                                └─ 门户页检测到代理已生效 → 绿色 ✓ "已连接"
└─ 电脑端实时显示「iPhone 已连接 (192.168.1.23)」Toast
    └─ 连接成功后自动弹出「抓 App 流量」技巧卡（SSL Pinning 提示等）
```

**关键机制**：
- **内置门户服务（Portal）**：代理在转发前拦截"目标是代理自身"的 HTTP 请求（修复 D1 的回环 bug），路由到内置门户页 `/portal`，提供证书下载（`/ca.crt`、`/ios.mobileconfig`、`/android.crt`）、代理配置指引、连接自检。
- **二维码**：UI 直接渲染 `http://{局域网IP}:{port}/portal` 的二维码（前端 qrcode 库，无需新后端依赖）。
- **客户端感知**：Rust 侧记录每个新建 TCP 连接的对端 IP，通过事件推送"新设备连接"通知，UI 侧边显示当前连接的客户端列表（IP / 平台推断 / 活跃请求数）。
- **iOS 描述文件**：动态生成 `.mobileconfig`（含 CA 证书 payload），用户安装后引导去"设置→通用→关于本机→证书信任设置"开启完全信任（页面给出跳转式图文步骤）。
- **adb reverse 自动化（可选增强）**：检测到本机 adb 与 Android 设备时，提供一键 `adb reverse tcp:PORT tcp:PORT`，手机用 `127.0.0.1:PORT` 即可，免 Wi-Fi 同网限制。
- **macOS 系统代理**：`networksetup -setwebproxy/-setsecurewebproxy Wi-Fi 127.0.0.1 port` 自动化（networksetup 是 BSD 许可系统自带，无需额外依赖），Linux 提供 GNOME/KDE 指引。

---

## 5. 功能规划总览

### P0 —— 地基：让它成为"可靠的日常工具"（里程碑 M1）

| 模块 | 内容 | 修复 |
|---|---|---|
| **门户服务** | 拦截自引用请求 → 门户页 + 证书分发 + 连接自检 + 二维码 | D1 |
| **客户端感知** | 设备连接事件 + 客户端列表面板 | — |
| **macOS/Linux 系统代理** | networksetup 自动化 + 恢复 | D6 |
| **WebSocket 解析** | 帧级记录：opcode/方向/时间/payload（文本/二进制），详情页时间线展示 | D3 |
| **SSE/chunked** | 流式响应实时推送（详情页"实时"标记），完整落盘 | — |
| **SQLite 落盘** | records 表 + 大 body 落文件（>256KB 存 blob 文件，DB 存引用），会话持久化、启动恢复 | D2 |
| **事件推送** | Tauri event（`traffic://new`、`traffic://update`）替代轮询；批量合并（100ms 窗口）避免洪峰 | D2 |
| **UI 重构** | 三栏布局（过滤导航栏 / 列表 / 详情）；详情 Tab 化；@tanstack/react-virtual 虚拟滚动；亮/暗主题跟随系统；中/英 i18n | D10 |
| **详情增强** | JSON 树形视图（可折叠/搜索/复制路径）、图片预览、Hex 视图、Cookie 解析、Query Params 表格、Timing 瀑布（dns/connect/tls/ttfb/download）、大小（请求/响应/解压后） | D5 |

### P1 —— 专业功能：规则与调试（里程碑 M2）

| 模块 | 内容 |
|---|---|
| **规则引擎** | 见 §6.5。匹配器（host/path/method/regex/header）× 动作（redirect / map-local / mock / rewrite / hosts / delay / abort / throttle / break），可视化编辑器 + 启用开关 + 优先级拖拽 + 导入导出 |
| **断点调试** | 请求/响应断点：命中后挂起 → UI 编辑（方法/URL/头/body）→ 放行/丢弃；支持"仅下次"断点 |
| **重放 Replay** | 修改后重发任意历史请求；diff 视图对比两次响应 |
| **Composer** | 从零构造 / 从历史请求派生，请求库收藏夹（集合管理） |
| **弱网模拟** | 限速（KB/s）、延迟（固定+抖动）、丢包率，按规则作用域应用；预设档位（2G/3G/弱 Wi-Fi）+ 自定义 |
| **导出导入** | HAR 1.2 导入/导出（含 bodies）；复制为 cURL；代码生成（fetch / axios / Python requests / Go net-http / OkHttp）；会话文件 `.paxi` 保存/打开 |
| **标注管理** | 标记颜色、备注、收藏；按标注过滤 |
| **AI 增强** | 上下文面板化（侧栏对话而非弹窗）；敏感头脱敏开关（默认开）；会话批量分析（挑 N 条让 AI 总结 API 行为）；自然语言→规则草稿 |

### P2 —— 深度与生态（里程碑 M3/M4）

| 模块 | 内容 | 里程碑 |
|---|---|---|
| **HTTP/2** | 客户端侧 h2（hyper `enable_http2`）+ 服务端内层 h2（h2 crate），流多路复用记录 | M3 |
| **统计分析** | 域名聚合面板（请求数/流量/平均耗时/错误率）、实时流量曲线（QPS/带宽）、慢请求 Top N | M3 |
| **上游代理链** | 级联公司代理（可配置上游 HTTP/SOCKS5），解决"公司内网必须走代理"场景 | M3 |
| **Diff 工具** | 任意两条请求/响应对比（头/体结构化 diff，JSON 语义 diff） | M3 |
| **脚本系统** | JS 规则脚本（boa/quickjs 沙箱嵌入 Rust）， onRequest/onResponse hooks，类 whistle 插件 | M4 |
| **Web 控制台** | 局域网只读 Web UI——手机浏览器实时看电脑上抓到的包（调试 App 时在手机上直接看请求） | M4 |
| **CLI 模式** | `paxi headless -p 8888 --filter "api" --dump out.har --rule rule.paxi`，CI/CD 集成 | M4 |
| **QUIC/HTTP3** | CONNECT-UDP / 阻断 QUIC 降级 H2 策略；HTTP/3 解析（远期） | M4+ |
| **TUN 透明代理** | 全局抓包（不依赖系统代理设置），wireguard-api/tun 方案（远期评估） | M4+ |

---

## 6. 关键模块详细设计

### 6.1 连接体验（P0 核心）

**门户服务（portal.rs）**
```
代理请求进入 handle_plain_http 时：
1. 解析目标 authority（host:port）
2. 若 host:port ∈ {本机所有网卡IP:监听端口, localhost, 127.0.0.1, paxi.local} 
   → 路由到 portal 路由表，不再转发
3. 路由表：
   GET /            → 门户页（自动识别 UA 渲染 iOS/Android/桌面指引）
   GET /ca.crt      → 根证书 DER（Windows 双击安装）
   GET /ca.pem      → 根证书 PEM
   GET /ios.mobileconfig → 动态生成的描述文件
   GET /android.crt → 根证书（Android 系统证书安装流）
   GET /download/favicon.ico 等 → 静态资源
   GET /ping        → 连接自检（返回 {"ok":true,"proxy":"IP:PORT"}）
   GET /health      → 门户可用性探测
```
- 门户页为内嵌静态资源（include_str! / include_bytes! 打进二进制），零外部依赖、离线可用。
- 页面极简：顶部大号代理地址 + 复制按钮 → 分平台安装卡（自动高亮当前 UA 平台）→ 底部"我已配置代理"自检按钮。
- iOS `.mobileconfig` 用 Rust 动态拼 plist（模板 + CA PEM），PayloadIdentifier 带随机串避免重复安装冲突。

**二维码（前端）**
- `qrcode.react` 或 `qrcode` npm 包；弹窗中同时展示：代理地址文本（一键复制）、证书直链二维码、操作步骤图。

**客户端感知（Rust）**
- `TcpStream.peer_addr()` → 客户端 IP 集合（LRU，5 分钟过期）；新 IP 首次出现 → `event::clients` 推送。
- UI：工具栏"已连接 N 台设备"，点开侧板：IP、平台（TTL/UA 粗判）、首连时间、活跃连接数。

### 6.2 抓包引擎（P0/P1）

**WebSocket（P0）**
- CONNECT TLS 解密后，若 HTTP 请求带 `Upgrade: websocket`：完成 101 握手后**不再走 HTTP service**，改用 `tokio-tungstenite` 双向转发并逐帧记录：
```rust
struct WsFrame { seq: u64, dir: Send|Recv, opcode: Text|Binary|Ping|Pong|Close, 
                 payload_len: u64, payload_text: Option<String>, ts_ms: u128 }
```
- 记录挂在所属 RequestRecord 上（列表里该请求显示 WS 徽标 + 帧计数）；详情页"Frames"Tab 时间线展示，二进制帧 Hex 查看。
- 断连时记录 Close 帧与原因码。

**大 body 与二进制（P0）**
- 内存阈值 256KB：小于→DB 存 TEXT/BLOB；大于→写入 `sessions/{sid}/bodies/{hash}` 文件，DB 存路径+sha256+size。
- 响应体不再截断丢失：列表 meta 永远轻量；详情按需读取（IPC 分页：offset/limit，图片/Hex 流式加载）。
- 图片（image/*）详情页直接预览；Protobuf 等未知二进制给 Hex + "保存到文件"。

**SSE / 流式（P0）**
- 不再 `body.collect()` 阻塞到完成：响应流式转发给客户端的同时 tee 一份写入存储；记录状态 `streaming → done`，UI 实时追加（详情页"Live"标记）。

**上游代理链（P2）**
- 设置项：`upstream: { type: http|socks5, host, port, auth? }`；上游生效时 CONNECT 转交上游建立、明文 HTTP 用绝对 URI 发上游。

### 6.3 存储与性能（P0）

**SQLite（rusqlite + WAL 模式）**
```sql
CREATE TABLE record (
  id TEXT PRIMARY KEY, session_id TEXT, started_at INTEGER, duration_ms INTEGER,
  method TEXT, url TEXT, host TEXT, scheme TEXT, status INTEGER,
  req_headers BLOB, resp_headers BLOB,
  req_body_ref TEXT, resp_body_ref TEXT,   -- 'inline:' 或 'file:路径'
  req_body_size INTEGER, resp_body_size INTEGER,
  content_type TEXT, error TEXT, is_websocket INTEGER,
  client_ip TEXT, flags INTEGER,           -- bit: 标注/收藏/断点命中
  note TEXT
);
CREATE INDEX idx_session_time ON record(session_id, started_at DESC);
CREATE INDEX idx_host ON record(host);
CREATE TABLE ws_frame (record_id TEXT, seq INTEGER, dir INTEGER, opcode TEXT,
                       payload BLOB, ts INTEGER);
CREATE TABLE session (id TEXT PRIMARY KEY, name TEXT, created_at INTEGER, count INTEGER);
CREATE TABLE rule (id TEXT PRIMARY KEY, enabled INTEGER, priority INTEGER, matcher BLOB, action BLOB, note TEXT);
```
- 会话（Session）概念：一次抓包任务=一个会话；启动时默认开新会话；UI 可切换/保存/导出会话。
- 保留策略：默认全保留，可设"仅保留 7 天 / 超过 N GB 提醒清理"。

**事件推送**
- Rust → 前端：`app_handle.emit("traffic://new", batch)`，100ms 窗口批量合并；列表只增量 append（zustand + 虚拟列表），彻底删除 2s 轮询。
- 状态机：`Provisional`（请求已发出）→ `Completed/Failed`，列表行原地更新状态。

### 6.4 UI/UX（P0）

**布局**
```
┌────────────────────────────────────────────────────────────────┐
│ Toolbar: [启动/停止] [端口] [记录数] [搜索框] [规则] [设备] [设置] │
├──────────┬──────────────────────────────────┬──────────────────┤
│ 导航栏    │ 流量列表（虚拟滚动）                │ 详情区            │
│ ▸ 全部   │ ┌─method─url─────────status─ms─┐ │ ┌Tab───────────┐ │
│ ▸ 域名树  │ │ GET  /api/users      200   32 │ │ Overview     │ │
│ ▸ 标注   │ │ POST /api/login      401   88 │ │ Headers      │ │
│ ▸ WS    │ │ …（10w 行 60fps）              │ │ Params       │ │
│ ▸ 规则   │ └───────────────────────────────┘ │ │ Body(Raw/    │ │
│ ▸ 收藏   │                                   │ │   Preview/Hex)│ │
│          │                                   │ │ Cookies      │ │
│          │                                   │ │ Timing       │ │
│          │                                   │ │ Frames(WS)   │ │
├──────────┴──────────────────────────────────┴──────────────────┤
│ 状态栏: 代理状态 · 当前过滤命中数 · SQLite 大小 · 崩溃恢复提示      │
└────────────────────────────────────────────────────────────────┘
```

**列表行**：彩色方法徽标、URL（host 灰 + path 白）、状态码色块（2xx 绿/3xx 蓝/4xx 橙/5xx 红/错误紫）、耗时、大小、协议/WS 徽标、标注色条；右键菜单：复制 URL/复制 cURL/重放/断点此域名/标注/保存 body。

**过滤**：搜索语法升级（`host:api.com method:POST status>=400 body:keyword` 组合），常用过滤保存为导航栏自定义项。

**快捷键**：`Ctrl+F` 聚焦搜索 / `↑↓` 列表导航 / `Enter` 打开详情 / `Space` Quick Look / `Ctrl+K` 命令面板 / `R` 重放 / `B` 设断点 / `Ctrl+Shift+R` 清空 / `Ctrl+E` 导出 HAR。

**主题与 i18n**：CSS variables 双主题，`prefers-color-scheme` 跟随 + 手动切换；i18n 框架（i18next）中/英。

### 6.5 规则引擎（P1）

**数据模型（Rust 端核心，前端可视化编辑）**
```rust
struct Rule {
  id: Uuid, enabled: bool, priority: i32, name: String,
  matcher: Matcher, action: Action,
}
struct Matcher {
  host: Option<Pattern>,        // 通配 *.api.com
  path: Option<Pattern>,        // 正则或 glob
  method: Option<Vec<String>>,
  query: Option<Pattern>,
  header: Option<(String, Pattern)>,
}
enum Action {
  Redirect { to: String, status: u16 },          // 302/307
  MapLocal { file: PathBuf, content_type: Option<String>, status: u16 },
  Mock { status: u16, headers: Vec<(String,String)>, body: Body },  // Body::Text|File|JsonTemplate
  RewriteRequest { replace_headers: .., body_find: Option<Regex>, body_replace: Option<String>, set_host: Option<String> },
  RewriteResponse { .. 同上, delay_ms: Option<u64> },
  Hosts { ip: String },                          // 域名映射 IP
  Delay { req_ms: u64, resp_ms: u64 },
  Abort,                                          // 直接断连（403 可选）
  Throttle { down_kbps: u32, up_kbps: u32, latency_ms: u32, loss_pct: u8 },
  Breakpoint { on: Request|Response|Both },
}
```
- 执行点：请求进入后按 priority 依次匹配（首个命中生效 or 可配置链式），对应 hook 点：`before_upstream_connect`（hosts/abort）、`before_send`（rewrite/delay/breakpoint）、`after_recv`（rewrite resp/delay/mock）、任意点（redirect/map-local 不发上游）。
- **可视化编辑器**：左列规则列表（拖拽排序、开关、命中计数），右侧编辑器（匹配条件表单 + 动作类型切换 + 参数表单）；"高级"折叠显示原始 JSON。
- **规则命中统计**：每条规则记录命中次数/最近命中时间，列表直接显示，方便清理僵尸规则。
- 规则集导入导出 JSON；后续兼容 whistle 规则语法子集导入（`pattern operator value` 文本）。

**断点实现**
- 命中后：记录状态置 `BreakpointPending`，请求 future 挂起在 `tokio::sync::oneshot` 通道上；UI 收到 `breakpoint://hit` 事件弹出编辑器；用户点"放行"→ IPC `breakpoint_resume(id, edited)` → 发送端收到修改后的请求继续。
- 超时策略：默认无限挂起（可在设置改 60s 自动放行）；应用退出时挂起请求统一放行并提示。

### 6.6 重放 / Composer（P1）
- Replay：从存储读原始请求（含保存的 body 文件）→ 预填编辑器 → 用户可改 → 发送（走代理自身引擎，规则同样生效，可选"忽略规则"开关）→ 结果追加到列表（带 `Replay` 徽标，可 diff 原请求）。
- Composer：表单式（方法/URL/头表格/body 类型选择：none/form/json/text/binary/file）+ 请求库（命名集合，localStorage + 可导出）。

### 6.7 导出与代码生成（P1）
- HAR 1.2：完整 headers/bodies/headersSize/bodySize/timings；导入时还原为会话记录。
- cURL 复制：注意 shell 脬义（bash/windows 两种模式）。
- 代码生成：纯前端模板（无后端参与）：fetch / axios / python-requests / go / okhttp / curl。
- `.paxi` 会话 = SQLite 单文件（就是会话 DB 本体），双击打开。

### 6.8 统计分析（P2）
- 域名面板：按 host 聚合（请求数、总流量、平均/95 分位耗时、错误率），点击钻取该域名全部请求。
- 流量曲线：实时 QPS + 上下行带宽（事件流驱动，canvas 绘制，60s/10min 两档）。
- 慢请求榜：P95 耗时 Top 20，一键定位。
- Waterfall：会话级时间线视图（每请求一横条，段染色：connect/tls/wait/download）。

### 6.9 AI 增强（P1 深化）
- 从弹窗改为**右侧常驻 Tab/分屏**：选中请求即可对话，上下文自动携带当前请求。
- 脱敏管道（发送前强制处理，UI 可查看脱敏后预览）：`Authorization/Cookie/Set-Cookie/x-token` 等头 → `***REDACTED***`；body 中命中 `(token|secret|password|key)` 的 JSON 字段值脱敏。
- 场景化按钮：解释此接口 / 推测参数含义 / 生成 TS 类型 / 生成调用代码 / 生成 OpenAPI 片段 / 分析签名算法（携带同域名多条请求对比 nonce/timestamp 变化）。
- 自然语言 → 规则草稿："把 api.test.com 的响应延迟 2 秒" → 解析为 Delay 规则，进编辑器待确认（不直接生效）。
- 批量分析：勾选 N 条 → 让 AI 总结这批接口的协议特征（版本号、签名位置、加密字段）。

### 6.10 安全与隐私（贯穿）
- CA 私钥：DPAPI（Win）/ Keychain（macOS，远期）加密存储，或至少文件权限 0600 + 首页提醒。
- 代理默认仅监听可信网卡；门户页局域网可达（这是功能需要），但提供"仅本机"开关。
- 敏感头脱敏展示开关（列表/详情/导出/HAR 四处一致生效）。
- AI 明示数据流向（发送到用户自配 API，不经过第三方）。

---

## 7. 技术架构

### 7.1 Rust 侧模块重组

```
src-tauri/src/
├── main.rs            # 入口
├── lib.rs             # Tauri commands（薄层，只做参数转发）
├── engine/
│   ├── mod.rs         # ProxyEngine：accept 循环、连接分发
│   ├── http1.rs       # HTTP/1 明文 + CONNECT
│   ├── tls.rs         # MITM TLS 握手 + 内层分发（h1 现有逻辑）
│   ├── h2.rs          # HTTP/2 内层（M3）
│   ├── ws.rs          # WebSocket 帧转发与记录（M1）
│   └── upstream.rs    # 上游代理链（M3）
├── portal.rs          # 门户服务 + mobileconfig 生成（M1）
├── rules/
│   ├── mod.rs         # 规则引擎：匹配 + 动作执行
│   ├── matcher.rs
│   └── actions.rs
├── breakpoint.rs      # 断点挂起/恢复管理（M2）
├── throttle.rs        # 限速/延迟/丢包（M2）
├── storage/
│   ├── mod.rs         # TrafficStore trait（内存 + SQLite 实现）
│   ├── sqlite.rs
│   └── bodies.rs      # 大 body 文件管理
├── export/
│   ├── har.rs         # HAR 导入导出
│   └── replay.rs      # 重放执行
├── ca.rs              # 现有，+ 加密存储改造
├── system_proxy.rs    # + macOS networksetup
├── clients.rs         # 客户端 IP 感知（M1）
└── events.rs          # 事件批量合并推送（M1）
```

### 7.2 数据流（M1 后）

```
客户端 ─TCP→ engine 分发 ─解密→ 规则引擎(匹配) ─→ upstream 发送
                │                              │
                │                       响应流式 tee
                ↓                              ↓
           storage(sqlite+bodies) ←──── record 状态机
                │
                ├→ events 批量(100ms) → 前端 store → 虚拟列表
                └→ 详情按需读（IPC get_body(id, offset, limit)）
```

### 7.3 新增依赖（预估）
- Rust：`rusqlite`(bundled)、`tokio-tungstenite`、`h2`（M3）、`regex`、`sha2`
- 前端：`@tanstack/react-virtual`、`qrcode`、`i18next` + `react-i18next`、`react-router`（多页面：流量/规则/统计/设置）

---

## 8. 里程碑

| 里程碑 | 周期 | 交付内容 | 验收标准（DoD） |
|---|---|---|---|
| **M1 地基** | 2 周 | 门户+二维码+证书分发、客户端感知、macOS 系统代理、WS 帧、SSE、SQLite+事件推送、UI 三栏重构（Tab/JSON 树/虚拟滚动/主题/i18n） | ① 手机扫码到装完证书≤3 步；② 10 万条记录滚动流畅；③ 杀进程重启数据还在；④ WS 帧在详情页可读；⑤ 暗色主题完整无破相 |
| **M2 规则与调试** | 3 周 | 规则引擎全动作、断点、Replay、Composer、弱网、HAR/cURL/代码生成、会话文件、标注、AI 面板化+脱敏 | ① Mock 一个接口不改代码；② 断点改 body 后放行生效；③ 重放带 diff；④ 导出 HAR 可被 Chrome DevTools 导入；⑤ AI 分析默认脱敏可预览 |
| **M3 深度** | 3 周 | HTTP/2、统计面板、上游代理链、Diff 工具 | ① H2 站点正常抓包且协议列显示 h2；② 域名聚合面板可用；③ 级联公司代理场景跑通 |
| **M4 生态** | 持续 | 脚本系统、Web 控制台、CLI、（远期）QUIC/TUN | 按子项独立验收 |

---

## 9. 风险与开放问题

| 风险 | 等级 | 对策 |
|---|---|---|
| HTTP/2 MITM 复杂度（流优先级、HPACK 动态表） | 高 | M3 单独 milestone；先客户端侧 h2（转 h1 记录）再服务端内层 h2 |
| SSL Pinning App 抓不了 | 中（业界共性） | 门户页/文档明示；远期评估 frida 集成指引而非内置（合规风险） |
| iOS 描述文件签名 | 低 | 无签名 mobileconfig 安装时会提示"未签名"，可接受；企业分发场景远期再评估 |
| SQLite 写入洪峰（压测场景 10k QPS） | 中 | WAL + 批量事务（与事件合并同 100ms 窗口）+ bodies 异步落盘；极端场景提供"仅内存模式"开关 |
| Tauri 多窗口/路由复杂化 | 低 | 单窗口 + 前端路由；保持简单 |
| 规则引擎与断点并发正确性 | 中 | 状态机单测覆盖；挂起请求在退出时统一放行的兜底 |

---

## 10. 立即行动（M1 第一周拆解）

1. 修复 D1：`handle_plain_http` 拦截自引用请求 → 挂载最小门户页（纯 HTML 证书下载）
2. 前端二维码弹窗（连接手机按钮）
3. `rusqlite` 接入 + TrafficStore trait 化（先并存内存实现，SQLite 成为默认）
4. events.rs 批量推送 + 删除前端轮询
5. WS：Upgrade 检测分支 + tungstenite 转发 + ws_frame 表
6. 三栏 UI 骨架 + 虚拟列表替换现列表
