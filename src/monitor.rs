//! 探测引擎：三条通道并行探测、阈值判定、历史采样、自动挑选内网目标。

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::config::{split_host_port, ChannelConfig, Config, Target, TargetKind};
use crate::json::Json;
use crate::probe::{self, HttpSession, ProbeResult};

const HISTORY_LEN: usize = 28;
const PRESENCE_TTL: Duration = Duration::from_secs(8);
const PEERS_TTL: Duration = Duration::from_secs(30);
const META_TTL: Duration = Duration::from_secs(300);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Good,
    Warn,
    Bad,
    Off,
    Unknown,
}

impl Level {
    pub fn key(self) -> &'static str {
        match self {
            Level::Good => "good",
            Level::Warn => "warn",
            Level::Bad => "bad",
            Level::Off => "off",
            Level::Unknown => "unknown",
        }
    }

    pub fn text(self) -> &'static str {
        match self {
            Level::Good => "正常",
            Level::Warn => "偏慢",
            Level::Bad => "不通",
            Level::Off => "未连接",
            Level::Unknown => "未知",
        }
    }

    pub fn severity(self) -> u8 {
        match self {
            Level::Unknown => 0,
            Level::Good => 1,
            Level::Off => 2,
            Level::Warn => 3,
            Level::Bad => 4,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ChannelState {
    pub key: &'static str,
    pub name: String,
    pub level: Level,
    pub latency_ms: Option<f64>,
    pub status: String,
    pub detail: String,
    pub history: Vec<Option<f64>>,
}

#[derive(Clone, Debug)]
pub struct Presence {
    pub connected: bool,
    pub label: String,
    pub ipv4: Vec<String>,
    pub gateway: Vec<String>,
    pub dns: Vec<String>,
}

impl Default for Presence {
    fn default() -> Self {
        Presence {
            connected: false,
            label: String::new(),
            ipv4: Vec::new(),
            gateway: Vec::new(),
            dns: Vec::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Snapshot {
    /// 本地时间字符串，报告里直接用
    pub stamp: String,
    pub channels: Vec<ChannelState>,
    pub proxy_enabled: bool,
    pub proxy_server: String,
    pub clash_meta: String,
    pub extra: Json,
}

impl Snapshot {
    pub fn channel(&self, key: &str) -> Option<&ChannelState> {
        self.channels.iter().find(|item| item.key == key)
    }

    pub fn overall(&self) -> (Level, String) {
        let mut level = Level::Unknown;
        let mut status = String::from("未知");
        for channel in &self.channels {
            if channel.level.severity() > level.severity() {
                level = channel.level;
                status = channel.status.clone();
            }
        }
        (level, status)
    }

    pub fn placeholder(config: &Config) -> Snapshot {
        let channels = vec![
            ChannelState {
                key: "clash",
                name: config.clash.name.clone(),
                level: Level::Unknown,
                latency_ms: None,
                status: "检测中…".into(),
                detail: String::new(),
                history: Vec::new(),
            },
            ChannelState {
                key: "vpn",
                name: config.vpn.name.clone(),
                level: Level::Unknown,
                latency_ms: None,
                status: "检测中…".into(),
                detail: String::new(),
                history: Vec::new(),
            },
            ChannelState {
                key: "direct",
                name: config.direct.name.clone(),
                level: Level::Unknown,
                latency_ms: None,
                status: "检测中…".into(),
                detail: String::new(),
                history: Vec::new(),
            },
        ];
        Snapshot {
            stamp: crate::ffi::win::local_time_string(),
            channels,
            proxy_enabled: false,
            proxy_server: String::new(),
            clash_meta: String::new(),
            extra: Json::Obj(Vec::new()),
        }
    }
}

fn level_of(latency_ms: f64, good_ms: f64, warn_ms: f64) -> Level {
    if latency_ms <= good_ms {
        Level::Good
    } else if latency_ms <= warn_ms {
        Level::Warn
    } else {
        Level::Bad
    }
}

fn status_of(level: Level) -> &'static str {
    match level {
        Level::Good => "正常",
        Level::Warn => "偏慢",
        Level::Bad => "很慢",
        Level::Off => "未连接",
        Level::Unknown => "未知",
    }
}

// ------------------------------------------------------------------ 通道基类
struct ChannelBase {
    config: ChannelConfig,
    history: VecDeque<Option<f64>>,
    preferred: String,
}

impl ChannelBase {
    fn new(config: ChannelConfig) -> ChannelBase {
        ChannelBase {
            config,
            history: VecDeque::with_capacity(HISTORY_LEN),
            preferred: String::new(),
        }
    }

    fn record(&mut self, value: Option<f64>) {
        if self.history.len() == HISTORY_LEN {
            self.history.pop_front();
        }
        self.history.push_back(value);
    }

    fn history(&self) -> Vec<Option<f64>> {
        self.history.iter().copied().collect()
    }

    fn ordered_targets(&self) -> Vec<Target> {
        let mut targets = self.config.targets.clone();
        if !self.preferred.is_empty() {
            if let Some(index) = targets.iter().position(|item| item.value == self.preferred) {
                let item = targets.remove(index);
                targets.insert(0, item);
            }
        }
        targets
    }

    fn state(
        &self,
        key: &'static str,
        level: Level,
        latency_ms: Option<f64>,
        status: String,
        detail: String,
    ) -> ChannelState {
        ChannelState {
            key,
            name: self.config.name.clone(),
            level,
            latency_ms,
            status,
            detail,
            history: self.history(),
        }
    }
}

// ------------------------------------------------------------------ HTTP 通道
pub struct HttpChannel {
    key: &'static str,
    base: ChannelBase,
    session: Option<HttpSession>,
}

impl HttpChannel {
    pub fn new(key: &'static str, config: ChannelConfig) -> HttpChannel {
        let session = HttpSession::new(&config.proxy);
        HttpChannel {
            key,
            base: ChannelBase::new(config),
            session,
        }
    }

    pub fn probe(&mut self, timeout: Duration, verify_default: bool) -> ChannelState {
        let good = self.base.config.good_ms;
        let warn = self.base.config.warn_ms;
        if !self.base.config.enabled {
            return self.base.state(
                self.key,
                Level::Off,
                None,
                "已停用".into(),
                "config.json 中已关闭".into(),
            );
        }
        if self.base.config.targets.is_empty() {
            return self.base.state(
                self.key,
                Level::Off,
                None,
                "未配置".into(),
                "请在 config.json 中配置探测地址".into(),
            );
        }

        let ordered = self.base.ordered_targets();
        let mut last: Option<ProbeResult> = None;
        let mut last_value = String::new();
        let mut proxy_down = !self.base.config.proxy.is_empty();

        for target in &ordered {
            if target.kind != TargetKind::Http {
                continue;
            }
            last_value = target.value.clone();
            let verify = target.verify.unwrap_or(verify_default);
            let result = match self.session.as_mut() {
                Some(session) => session.get(&target.value, timeout, verify),
                None => ProbeResult::fail("HTTP 会话初始化失败"),
            };
            if result.ok {
                let latency = result.latency_ms.unwrap_or(0.0);
                self.base.preferred = target.value.clone();
                self.base.record(Some(latency));
                let level = level_of(latency, good, warn);
                return self.base.state(
                    self.key,
                    level,
                    Some(latency),
                    status_of(level).to_string(),
                    format!("{} · {}", probe::host_of(&target.value), result.detail),
                );
            }
            if !result.proxy_down {
                proxy_down = false;
            }
            last = Some(result);
        }

        self.base.record(None);
        match last {
            Some(_) if proxy_down => self.base.state(
                self.key,
                Level::Off,
                None,
                "代理未启动".into(),
                self.base.config.proxy.clone(),
            ),
            Some(result) => self.base.state(
                self.key,
                Level::Bad,
                result.latency_ms,
                result.detail,
                probe::host_of(&last_value),
            ),
            None => self.base.state(
                self.key,
                Level::Off,
                None,
                "未配置".into(),
                "没有可用的 HTTP 目标".into(),
            ),
        }
    }
}

// ------------------------------------------------------------------ 内网通道
pub struct VpnChannel {
    base: ChannelBase,
    session: Option<HttpSession>,
    keywords: Vec<String>,
    process_names: Vec<String>,
    presence: Presence,
    presence_at: Option<Instant>,
    peers: Vec<Target>,
    peers_at: Option<Instant>,
}

impl VpnChannel {
    pub fn new(config: ChannelConfig, keywords: Vec<String>, process_names: Vec<String>) -> VpnChannel {
        let session = HttpSession::new("");
        VpnChannel {
            base: ChannelBase::new(config),
            session,
            keywords,
            process_names,
            presence: Presence::default(),
            presence_at: None,
            peers: Vec::new(),
            peers_at: None,
        }
    }

    pub fn presence(&mut self) -> Presence {
        if let Some(at) = self.presence_at {
            if at.elapsed() < PRESENCE_TTL {
                return self.presence.clone();
            }
        }
        let mut found = Presence::default();
        for adapter in probe::adapters() {
            let haystack = format!("{} {}", adapter.name, adapter.description).to_lowercase();
            let matched = self
                .keywords
                .iter()
                .find(|keyword| haystack.contains(&keyword.to_lowercase()));
            if let Some(_keyword) = matched {
                found = Presence {
                    connected: adapter.up,
                    label: if adapter.description.is_empty() {
                        adapter.name.clone()
                    } else {
                        adapter.description.clone()
                    },
                    ipv4: adapter.ipv4.clone(),
                    gateway: adapter.gateway.clone(),
                    dns: adapter.dns.clone(),
                };
                break;
            }
        }
        if found.label.is_empty() {
            let running = probe::running_processes(&self.process_names);
            if let Some(name) = running.first() {
                found.connected = true;
                found.label = name.clone();
            }
        }
        self.presence = found.clone();
        self.presence_at = Some(Instant::now());
        found
    }

    fn auto_targets(&mut self, presence: &Presence) -> Vec<Target> {
        let mut targets: Vec<Target> = Vec::new();
        let fresh = self
            .peers_at
            .map(|at| at.elapsed() < PEERS_TTL)
            .unwrap_or(false);
        if !fresh {
            let peers = probe::tcp_peers(&presence.ipv4, 8);
            self.peers = peers
                .into_iter()
                .filter(|(ip, _)| crate::ffi::is_private_ipv4(ip))
                .take(3)
                .map(|(ip, port)| Target {
                    kind: TargetKind::Icmp,
                    value: ip.clone(),
                    name: format!("内网 {}:{}", ip, port),
                    verify: None,
                    port: Some(port),
                })
                .collect();
            self.peers_at = Some(Instant::now());
        }
        targets.extend(self.peers.iter().cloned());

        if let Some(dns) = presence.dns.first() {
            targets.push(Target {
                kind: TargetKind::Icmp,
                value: dns.clone(),
                name: format!("DNS {}", dns),
                verify: None,
                port: None,
            });
        }
        if let Some(gateway) = presence.gateway.first() {
            targets.push(Target {
                kind: TargetKind::Icmp,
                value: gateway.clone(),
                name: format!("网关 {}", gateway),
                verify: None,
                port: None,
            });
        }
        if targets.is_empty() {
            if let Some(ip) = presence.ipv4.first() {
                if let Some((prefix, _)) = ip.rsplit_once('.') {
                    let guess = format!("{}.1", prefix);
                    if &guess != ip {
                        targets.push(Target {
                            kind: TargetKind::Icmp,
                            value: guess.clone(),
                            name: format!("网关 {}", guess),
                            verify: None,
                            port: None,
                        });
                    }
                }
            }
        }
        targets.truncate(4);
        targets
    }

    fn probe_target(
        &mut self,
        target: &Target,
        timeout: Duration,
        verify_default: bool,
    ) -> Option<ProbeResult> {
        match target.kind {
            TargetKind::Http => {
                let verify = target.verify.unwrap_or(verify_default);
                self.session
                    .as_mut()
                    .map(|session| session.get(&target.value, timeout, verify))
            }
            TargetKind::Tcp => {
                let (host, port) = split_host_port(&target.value)?;
                Some(probe::tcp_probe(&host, port, timeout))
            }
            TargetKind::Icmp => Some(probe::icmp_probe(
                &target.value,
                timeout.min(Duration::from_millis(2000)),
            )),
        }
    }

    /// 候选目标并行探测，取延迟最低的成功者；ICMP 不通时回退 TCP。
    fn best_of(
        &mut self,
        targets: &[Target],
        timeout: Duration,
    ) -> Option<(String, ProbeResult)> {
        let mut best: Option<(String, ProbeResult)> = None;
        std::thread::scope(|scope| {
            let mut handles = Vec::new();
            for target in targets {
                let label = target.label();
                handles.push((
                    label,
                    scope.spawn(move || -> Option<ProbeResult> {
                        let result = match target.kind {
                            TargetKind::Tcp => {
                                let (host, port) = split_host_port(&target.value)?;
                                probe::tcp_probe(&host, port, timeout)
                            }
                            TargetKind::Icmp => probe::icmp_probe(
                                &target.value,
                                timeout.min(Duration::from_millis(2000)),
                            ),
                            TargetKind::Http => return None,
                        };
                        if result.ok {
                            return Some(result);
                        }
                        // 内网主机常常禁 ping，用较短的超时快速退回 TCP
                        if let Some(port) = target.port {
                            let tcp_timeout = timeout.min(Duration::from_millis(1200));
                            return Some(probe::tcp_probe(&target.value, port, tcp_timeout));
                        }
                        Some(result)
                    }),
                ));
            }
            for (label, handle) in handles {
                if let Ok(Some(result)) = handle.join() {
                    if !result.ok {
                        continue;
                    }
                    let latency = result.latency_ms.unwrap_or(f64::MAX);
                    let better = best
                        .as_ref()
                        .map(|(_, current)| latency < current.latency_ms.unwrap_or(f64::MAX))
                        .unwrap_or(true);
                    if better {
                        best = Some((label, result));
                    }
                }
            }
        });
        best
    }

    pub fn probe(&mut self, timeout: Duration, verify_default: bool) -> ChannelState {
        let good = self.base.config.good_ms;
        let warn = self.base.config.warn_ms;
        if !self.base.config.enabled {
            return self.base.state(
                "vpn",
                Level::Off,
                None,
                "已停用".into(),
                "config.json 中已关闭".into(),
            );
        }
        let presence = self.presence();
        let label = if presence.label.is_empty() {
            "未检测到 VPN 网卡/进程".to_string()
        } else {
            presence.label.clone()
        };
        let local_ip = presence.ipv4.first().cloned().unwrap_or_default();
        let prefix = if local_ip.is_empty() {
            String::new()
        } else {
            format!("{} · ", local_ip)
        };

        let manual = !self.base.config.targets.is_empty();
        let targets = if manual {
            self.base.ordered_targets()
        } else {
            self.auto_targets(&presence)
        };

        if targets.is_empty() {
            self.base.record(None);
            return if presence.connected {
                self.base.state("vpn", Level::Good, None, "已连接".into(), format!("{}{}", prefix, label))
            } else {
                self.base.state("vpn", Level::Off, None, "未连接".into(), label)
            };
        }

        if !manual {
            let best = self.best_of(&targets, timeout);
            if let Some((name, result)) = best {
                let latency = result.latency_ms.unwrap_or(0.0);
                self.base.record(Some(latency));
                let level = level_of(latency, good, warn);
                return self.base.state(
                    "vpn",
                    level,
                    Some(latency),
                    status_of(level).to_string(),
                    format!("{}{} {}", prefix, name, result.detail),
                );
            }
            self.base.record(None);
            return if presence.connected {
                self.base.state(
                    "vpn",
                    Level::Warn,
                    None,
                    "已连接".into(),
                    format!("{}未测得内网延迟，可在 config.json 配置 vpn.targets", prefix),
                )
            } else {
                self.base.state("vpn", Level::Off, None, "未连接".into(), label)
            };
        }

        let mut last_detail = String::new();
        for target in &targets {
            let result = self.probe_target(target, timeout, verify_default);
            match result {
                None => last_detail = "目标配置无效".to_string(),
                Some(value) if value.ok => {
                    let latency = value.latency_ms.unwrap_or(0.0);
                    self.base.preferred = target.value.clone();
                    self.base.record(Some(latency));
                    let level = level_of(latency, good, warn);
                    return self.base.state(
                        "vpn",
                        level,
                        Some(latency),
                        status_of(level).to_string(),
                        format!("{}{} {}", prefix, target.value, value.detail),
                    );
                }
                Some(value) => last_detail = value.detail,
            }
        }

        self.base.record(None);
        if presence.connected {
            let status = if last_detail.is_empty() {
                "内网不可达".to_string()
            } else {
                last_detail
            };
            self.base.state("vpn", Level::Bad, None, status, prefix + &label)
        } else {
            self.base.state("vpn", Level::Off, None, "未连接".into(), label)
        }
    }
}

// ------------------------------------------------------------------ 引擎
pub struct Engine {
    pub config: Config,
    clash: HttpChannel,
    vpn: VpnChannel,
    direct: HttpChannel,
    meta_shared: Arc<Mutex<Option<String>>>,
    meta_busy: Arc<AtomicBool>,
    clash_meta: String,
    clash_meta_at: Option<Instant>,
    last: Snapshot,
    /// 上一轮各阶段耗时（毫秒），用于自测与排查
    pub last_timing: Vec<(&'static str, f64)>,
}

impl Engine {
    pub fn new(config: Config) -> Engine {
        let clash = HttpChannel::new("clash", config.clash.clone());
        let direct = HttpChannel::new("direct", config.direct.clone());
        let vpn = VpnChannel::new(
            config.vpn.clone(),
            config.adapter_keywords.clone(),
            config.process_names.clone(),
        );
        let last = Snapshot::placeholder(&config);
        Engine {
            meta_shared: Arc::new(Mutex::new(None)),
            meta_busy: Arc::new(AtomicBool::new(false)),
            config,
            clash,
            vpn,
            direct,
            clash_meta: String::new(),
            clash_meta_at: None,
            last,
            last_timing: Vec::new(),
        }
    }

    pub fn last(&self) -> Snapshot {
        self.last.clone()
    }

    pub fn reload(&mut self) {
        let path = self.config.path.clone();
        let config = Config::load(&path);
        self.config = config;
        self.clash = HttpChannel::new("clash", self.config.clash.clone());
        self.direct = HttpChannel::new("direct", self.config.direct.clone());
        self.vpn = VpnChannel::new(
            self.config.vpn.clone(),
            self.config.adapter_keywords.clone(),
            self.config.process_names.clone(),
        );
        self.clash_meta = String::new();
        self.clash_meta_at = None;
        if let Ok(mut guard) = self.meta_shared.lock() {
            *guard = None;
        }
        self.meta_busy.store(false, Ordering::Relaxed);
    }

    /// Clash 版本/模式只是锦上添花，放到后台线程查，绝不阻塞探测轮。
    fn update_meta(&mut self) {
        self.clash_meta = self
            .meta_shared
            .lock()
            .map(|guard| guard.clone().unwrap_or_default())
            .unwrap_or_default();

        let fresh = self
            .clash_meta_at
            .map(|at| at.elapsed() < META_TTL)
            .unwrap_or(false);
        if fresh || self.meta_busy.load(Ordering::Relaxed) || self.config.clash.controller.is_empty()
        {
            return;
        }
        self.clash_meta_at = Some(Instant::now());
        self.meta_busy.store(true, Ordering::Relaxed);

        let controller = self.config.clash.controller.trim_end_matches('/').to_string();
        let shared = Arc::clone(&self.meta_shared);
        let busy = Arc::clone(&self.meta_busy);
        std::thread::spawn(move || {
            // 先用毫秒级 TCP 预检：控制器没开时直接跳过，
            // 否则 WinHTTP 连被拒端口会反复重试，白烧几秒 CPU
            let text = if controller_reachable(&controller) {
                query_clash_meta(&controller)
            } else {
                String::new()
            };
            if let Ok(mut guard) = shared.lock() {
                *guard = Some(text);
            }
            busy.store(false, Ordering::Relaxed);
        });
    }

    pub fn refresh(&mut self) -> Snapshot {
        let timeout = Duration::from_secs_f64(self.config.timeout);
        let verify = self.config.verify_ssl;
        let round_started = Instant::now();
        let clash = &mut self.clash;
        let vpn = &mut self.vpn;
        let direct = &mut self.direct;
        let (clash_state, vpn_state, direct_state) = std::thread::scope(|scope| {
            let a = scope.spawn(move || clash.probe(timeout, verify));
            let b = scope.spawn(move || vpn.probe(timeout, verify));
            let c = scope.spawn(move || direct.probe(timeout, verify));
            (
                a.join().unwrap_or_else(|_| unreachable_state("clash", "探测线程崩溃")),
                b.join().unwrap_or_else(|_| unreachable_state("vpn", "探测线程崩溃")),
                c.join().unwrap_or_else(|_| unreachable_state("direct", "探测线程崩溃")),
            )
        });
        let probe_ms = round_started.elapsed().as_secs_f64() * 1000.0;

        let meta_started = Instant::now();
        self.update_meta();
        let meta_ms = meta_started.elapsed().as_secs_f64() * 1000.0;

        let post_started = Instant::now();
        let (proxy_enabled, proxy_server) = probe::system_proxy();
        let presence = self.vpn.presence();
        let extra = self.context(&presence);
        let post_ms = post_started.elapsed().as_secs_f64() * 1000.0;

        self.last_timing = vec![
            ("三通道并行探测", probe_ms),
            ("Clash 控制器查询", meta_ms),
            ("环境信息采集", post_ms),
        ];

        let snapshot = Snapshot {
            stamp: crate::ffi::win::local_time_string(),
            channels: vec![clash_state, vpn_state, direct_state],
            proxy_enabled,
            proxy_server,
            clash_meta: self.clash_meta.clone(),
            extra,
        };
        self.last = snapshot.clone();
        snapshot
    }

    fn context(&self, presence: &Presence) -> Json {
        let targets = |channel: &ChannelBase| -> Vec<Json> {
            channel
                .config
                .targets
                .iter()
                .map(|item| Json::Str(item.value.clone()))
                .collect()
        };
        let vpn_targets = if self.vpn.base.config.targets.is_empty() {
            vec![Json::Str("(未配置，按 VPN 网卡/DNS/网关自动探测)".into())]
        } else {
            targets(&self.vpn.base)
        };
        let thresholds = Json::Obj(vec![
            (
                "clash".into(),
                Json::Obj(vec![
                    ("good".into(), Json::Num(self.clash.base.config.good_ms)),
                    ("warn".into(), Json::Num(self.clash.base.config.warn_ms)),
                ]),
            ),
            (
                "vpn".into(),
                Json::Obj(vec![
                    ("good".into(), Json::Num(self.vpn.base.config.good_ms)),
                    ("warn".into(), Json::Num(self.vpn.base.config.warn_ms)),
                ]),
            ),
            (
                "direct".into(),
                Json::Obj(vec![
                    ("good".into(), Json::Num(self.direct.base.config.good_ms)),
                    ("warn".into(), Json::Num(self.direct.base.config.warn_ms)),
                ]),
            ),
        ]);
        Json::Obj(vec![
            (
                "vpn".into(),
                Json::Obj(vec![
                    ("adapter".into(), Json::Str(presence.label.clone())),
                    ("connected".into(), Json::Bool(presence.connected)),
                    (
                        "ipv4".into(),
                        Json::Arr(presence.ipv4.iter().cloned().map(Json::Str).collect()),
                    ),
                    (
                        "gateway".into(),
                        Json::Arr(presence.gateway.iter().cloned().map(Json::Str).collect()),
                    ),
                    (
                        "dns".into(),
                        Json::Arr(presence.dns.iter().cloned().map(Json::Str).collect()),
                    ),
                ]),
            ),
            (
                "targets".into(),
                Json::Obj(vec![
                    ("clash".into(), Json::Arr(targets(&self.clash.base))),
                    ("vpn".into(), Json::Arr(vpn_targets)),
                    ("direct".into(), Json::Arr(targets(&self.direct.base))),
                ]),
            ),
            ("thresholds".into(), thresholds),
            ("interval_seconds".into(), Json::Num(self.config.interval)),
        ])
    }
}

/// 控制器端口是否有人监听（避免对着被拒绝的端口做完整 HTTP 尝试）。
fn controller_reachable(controller: &str) -> bool {
    use std::net::ToSocketAddrs;

    let parsed = match crate::probe::parse_url(controller) {
        Some(value) => value,
        None => return false,
    };
    let address = match (parsed.host.as_str(), parsed.port)
        .to_socket_addrs()
        .ok()
        .and_then(|mut iter| iter.next())
    {
        Some(value) => value,
        None => return false,
    };
    std::net::TcpStream::connect_timeout(&address, Duration::from_millis(250)).is_ok()
}

/// 读取 Clash external-controller 的版本与模式。
fn query_clash_meta(controller: &str) -> String {
    let mut session = match HttpSession::new("") {
        Some(value) => value,
        None => return String::new(),
    };
    let timeout = Duration::from_millis(1500);
    let version = session.get_text(&format!("{}/version", controller), timeout);
    let configs = session.get_text(&format!("{}/configs", controller), timeout);
    let version_text = version
        .body
        .as_deref()
        .and_then(|text| crate::json::Json::parse(text).ok())
        .map(|json| json.string("version", ""))
        .unwrap_or_default();
    if version_text.is_empty() {
        return String::new();
    }
    let mode = configs
        .body
        .as_deref()
        .and_then(|text| crate::json::Json::parse(text).ok())
        .map(|json| json.string("mode", ""))
        .unwrap_or_default();
    let mode_text = match mode.as_str() {
        "rule" => "规则",
        "global" => "全局",
        "direct" => "直连",
        other => other,
    };
    format!("Clash {} · {} 模式", version_text, mode_text)
}

fn unreachable_state(key: &'static str, message: &str) -> ChannelState {
    ChannelState {
        key,
        name: key.to_string(),
        level: Level::Unknown,
        latency_ms: None,
        status: "探测异常".into(),
        detail: message.to_string(),
        history: Vec::new(),
    }
}
