# 办公网络状态监测 · Windows 托盘小工具

在任务栏用一个小图标同时盯住三条链路，一眼看出「外网走没走通、内网 VPN 连没连、国内直连卡不卡」：

| 通道 | 回答什么问题 | 判定方式 |
| --- | --- | --- |
| 外网 · Clash | 还能不能正常访问境外服务 | 走 `127.0.0.1:7890` 代理请求 `generate_204`，量「发出 → 收到响应头」的耗时 |
| 内网 · OpenVPN | 还能不能打开公司内网系统 | 先判断 VPN 网卡/进程，再探测内网目标（未配置时自动找，见下） |
| 国内 · 直连 | 我的宽带和国内网站本身好不好 | 显式绕过系统代理请求百度/QQ，量耗时 |

用 Rust + 纯系统 API 实现，**零第三方依赖**：

| 指标 | 实测 |
| --- | --- |
| 内存 · 私有 | 冷启动 5~12 MB，跑热后约 16.6 MB |
| 内存 · 工作集 | 冷启动 16~25 MB，跑热后约 36 MB（含共享 DLL 页） |
| CPU | 0.3 ~ 0.6% 单核（12 核机器上约整机 0.03%） |
| 泄漏检查 | 面板开着 60 秒（重绘 12 次）私有 +0.0 MB、句柄 ±2 |
| 单文件 exe | 约 0.38 MB |

作为参照：Clash for Windows 常驻 125 MB、OpenVPN Connect 89 MB、explorer 126 MB —— 这个小工具比它监控的两个对象都轻。

## 使用

- **双击 exe 即可**，无需安装任何运行时。首次运行会在 exe 同目录生成 `config.json`。
- 想换位置就把 exe 挪到任意文件夹，配置文件跟着重新生成。
- 开机自启：`Win + R` 输入 `shell:startup`，把 exe 的快捷方式丢进去。
- 退出请走托盘右键菜单的「退出」；重复启动会被单实例锁挡掉并提示。

### 托盘图标

圆环被切成三段，上=外网、右下=内网、左下=国内；圆心是总体状态（取三条中最差的一条）。

| 颜色 | 含义 |
| --- | --- |
| 绿 `#3DDC84` | 正常（≤ 该通道 `good_ms`） |
| 黄 `#FFB648` | 偏慢（≤ `warn_ms`） |
| 红 `#FF5F56` | 极慢或不通 |
| 灰 `#7C8798` | 未连接 / 未启用 / 未配置 |

鼠标悬停显示一行摘要，如 `外网 √ 128ms · 内网 ○ — · 国内 √ 18ms`。

### 状态面板

左键点击托盘图标弹出：

```
┌──────────────────────────────────┐
│ 网络状态          更新于 13:22:41 ✕│
├──────────────────────────────────┤
│ ● 外网 · Clash          130 ms   │
│   正常 · www.gstatic.com   ▂▃▂▃  │
├──────────────────────────────────┤
│ ● 内网 · OpenVPN         32 ms   │
│   正常 · 10.17.232.44 网关…  ▂▂▂  │
├──────────────────────────────────┤
│ ● 国内 · 直连             48 ms   │
│   正常 · www.baidu.com     ▂▂▃▂  │
├──────────────────────────────────┤
│ 系统代理 127.0.0.1:7890 · 接口 …  │
│ [  立即刷新  ] [  复制报告  ]      │
└──────────────────────────────────┘
```

- 每张卡片：状态点、通道名、最近一次延迟、迷你波形（最近 14 次采样）
- 底部两个按钮：立即刷新、复制报告（复制进剪贴板，可直接贴给大模型）
- **Esc / 点 ✕ / 点面板外面**都能收起；换 DPI 会按缩放重绘

## 配置

`config.json` 在 exe 同目录（托盘右键「编辑配置」可直接打开，改完点「重载配置」生效，无需重启）。

```jsonc
{
  "interval_seconds": 5,      // 探测间隔
  "timeout_seconds": 2.5,     // 单次探测超时
  "verify_ssl": true,         // 内网站点若用自签证书，改成 false
  "clash": {
    "enabled": true,
    "name": "外网 · Clash",
    "proxy": "http://127.0.0.1:7890",        // Clash 的 mixed-port / http-port
    "controller": "http://127.0.0.1:9090",   // external-controller，用于显示版本与模式（可选）
    "targets": ["https://www.gstatic.com/generate_204"],
    "good_ms": 600,
    "warn_ms": 1500
  },
  "vpn": {
    "enabled": true,
    "name": "内网 · OpenVPN",
    "adapter_keywords": ["TAP-Windows", "OpenVPN", "Wintun", "TUN"],
    "process_names": ["openvpn.exe", "openvpn-gui.exe"],
    "targets": [],            // 留空 = 自动探测
    "good_ms": 150,
    "warn_ms": 500
  },
  "direct": {
    "enabled": true,
    "name": "国内 · 直连",
    "targets": ["https://www.baidu.com", "https://www.qq.com"],
    "good_ms": 300,
    "warn_ms": 800
  },
  "ai_bridge": {              // 本机只读接口
    "enabled": true,
    "host": "127.0.0.1",
    "port": 17800,
    "token": ""               // 非空时要求 ?token= 或 X-Token
  }
}
```

说明：

- `targets` 按顺序探测，任意一个成功即视为该通道正常；字符串按 HTTP 处理，也可以写成对象指定类型：
  ```json
  { "type": "icmp", "target": "10.17.232.1" }
  { "type": "tcp",  "target": "10.17.232.1:3389" }
  { "type": "http", "target": "http://10.10.0.1:8080/health", "verify": false }
  ```
- **内网 `targets` 留空时会自动找探测点**，按优先级：
  1. 当前正通过 VPN 网卡通信的**内网服务器**（从系统 TCP 表里挖，自动过滤公网地址 —— 全隧道模式下 VPN 网卡也承载公网流量，不过滤就会误判）
  2. VPN 网卡下发的 DNS
  3. 同网段网关（`x.y.z.1`）
  
  这些候选并行探测、取最快的一个；ICMP 不通会自动退回 TCP。全部无响应则亮黄灯如实报「未测得延迟」，不假装绿。
- 想要最准的内网延迟，建议手动填一个你常用的内网系统（OA、GitLab、文件服务器都行）。

## 交给大模型分析

### 1. 一键复制报告

面板里的「复制报告」（或托盘右键「复制诊断报告」）会把当前状态整理成结构化 Markdown 放进剪贴板，内容全是事实、不做推断，直接粘给任何大模型即可：

```markdown
# 办公网络状态报告
- 生成时间：2026-09-20 13:30:32（采样间隔 5 秒，每条通道保留最近 28 次）
- 总体状态：正常

| 通道 | 状态 | 最近延迟 | 成功率 | 最小/中位/最大 | 抖动 | 说明 |
| --- | --- | --- | --- | --- | --- | --- |
| 外网 · Clash | 🟢 正常 | 136 ms | 28/28 | 130.4/142.1/799.6 | 669.2 | www.gstatic.com · HTTP 204 |
...

## 本机环境
- 系统代理：开启（127.0.0.1:7890）
- VPN 网卡：TAP-Windows Adapter V9 for OpenVPN Connect（本机 10.17.232.44，网关 -，DNS -）
- vpn 探测目标：(未配置，按 VPN 网卡/DNS/网关自动探测)
- 判定阈值：clash 正常≤600ms、偏慢≤1500ms；vpn 正常≤150ms、偏慢≤500ms；direct 正常≤300ms、偏慢≤800ms

## 原始采样（ms，null 表示该次失败）
- 外网 · Clash：[136, 799, null, ...]
```

### 2. 本机只读接口

只监听 `127.0.0.1`，不对外暴露、也不会主动联网：

| 地址 | 内容 |
| --- | --- |
| `http://127.0.0.1:17800/status` | 当前状态 JSON（含统计与环境信息），适合脚本 / Agent 调用 |
| `http://127.0.0.1:17800/report` | Markdown 报告，内容与「复制报告」一致 |
| `http://127.0.0.1:17800/` | 说明页 |

### 给模型的提示词示例

```text
以下是我当前办公网络的监测数据：外网走 Clash 代理、内网走 OpenVPN、国内站点直连。
请判断：
1. 三条链路是否正常，延迟水平相对阈值是否异常
2. 是否存在抖动过大、丢包失败、间歇性超时的迹象
3. 最可能的原因（代理节点慢 / VPN 隧道抖动 / DNS 异常 / 本机网络不稳），按可能性排序
4. 给出下一步具体排查动作

<把「复制报告」的内容粘贴到这里>
```

## 编译

需要 Rust（含 MSVC 链接器）。**不依赖任何第三方 crate，编译不需要联网。**

```powershell
cargo build --release                              # 产物 target\release\NetworkMonitor.exe（无控制台）
cargo build --release --features console           # 保留控制台窗口，便于排查
.\target\release\NetworkMonitor.exe --selftest     # 控制台自测：打印三轮探测结果与各阶段耗时
.\target\release\NetworkMonitor.exe --show-panel   # 启动后自动弹出面板（自检用）
```

exe 图标在构建时由 `build.rs` 调用 Windows SDK 的 `rc.exe` 嵌入（找不到就跳过，可用 `RC_EXE` 指定）。

## 代码结构

```
src/main.rs      入口：单实例、DPI 感知、消息循环、探测线程调度
src/monitor.rs   三条通道的聚合与判定：阈值分档、历史采样、自动挑选内网目标
src/probe.rs     探测层：WinHTTP / ICMP / TCP / 网卡 / TCP 对端 / 系统代理
src/tray.rs      托盘：图标、悬浮提示、右键菜单、剪贴板
src/panel.rs     面板：分层窗口 + GDI+ 绘制、命中测试、失焦收起
src/bridge.rs    本机只读接口（/status、/report）
src/report.rs    快照 -> JSON / Markdown
src/json.rs      迷你 JSON 解析与序列化
src/gdiplus.rs   GDI+ 封装：圆角、圆弧、文字、位图
src/ffi/         Win32 / WinHTTP / IP Helper / 注册表 声明
src/process.rs   进程枚举（Toolhelp32）
src/config.rs    配置读写与默认值
```

## 设计说明与踩过的坑

**为什么这么省内存**

- HTTP 走 **WinHTTP**：自带连接池，证书用系统证书库（不需要像 Python 版那样带一份 CA 包）
- ICMP 用 **IcmpSendEcho**：不再 fork `ping.exe`，也绕开了中文 Windows 的 GBK 输出编码坑
- 托盘与面板都是原生窗口，没有解释器和 GUI 框架的底座
- 配置解析自己写了 200 行 JSON，一个 crate 都不引

**踩过的坑（都已修）**

- **WinHTTP 连不上被拒端口时会反复重试**：对着没在监听的 Clash 控制器端口做完整请求会烧掉十几秒 CPU。现在先用 250ms 的 TCP 预检，端口没人监听就直接跳过。
- **控制器查询不能放在探测轮里**：它是锦上添花的信息，放到后台线程做，永不阻塞探测。
- **新建窗口会先收到一次 `WM_ACTIVATE(WA_INACTIVE)`**：如果直接当成"失焦"就收起，面板会一弹出就闪掉。现在只有「拿到过焦点后的失焦」或超过 400ms 宽限期才收起。
- **`SetForegroundWindow` 会被系统拒绝**：由托盘点击这种程序内部消息触发抢前台，Windows 不认账，面板就拿不到焦点、"点外部收起"永远不触发。解法是先 `AttachThreadInput` 挂到当前前台线程再抢。
- **分层窗口必须用预乘 alpha**：GDI+ 要包 `PIXEL_FORMAT_32BPP_PARGB` 的 DIB 内存，否则半透明边缘会发灰。
- **`GetAdaptersAddresses` 不传 `GAA_FLAG_INCLUDE_GATEWAYS` 时，网关链表恒为空**。
- **`GetExtendedTcpTable` 的端口是网络字节序**，且只占低 16 位。

**已知取舍**

- 报的是**延迟 / 抖动 / 成功率**，不是带宽（测吞吐要下载大文件，代价大且会污染你正在用的网络）
- HTTPS 走 CONNECT 隧道，外网那条测到的是「到代理 + 到目标」的合成时间，分不出是哪一段慢
- Clash 若用 TUN 模式，国内直连也过它的网卡，只是规则分流到 DIRECT，测到的仍是真实体感
- 5 秒一次采样看趋势，抓不到毫秒级的瞬时卡顿
