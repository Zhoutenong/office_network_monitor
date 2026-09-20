//! 配置：与 Python 版同名的 config.json，字段一一对应。

use std::path::{Path, PathBuf};

use crate::json::Json;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TargetKind {
    Http,
    Tcp,
    Icmp,
}

#[derive(Debug, Clone)]
pub struct Target {
    pub kind: TargetKind,
    pub value: String,
    pub name: String,
    pub verify: Option<bool>,
    /// ICMP 目标不通时回退的 TCP 端口（内网主机常常禁 ping）
    pub port: Option<u16>,
}

impl Target {
    pub fn label(&self) -> String {
        if !self.name.is_empty() {
            return self.name.clone();
        }
        match self.kind {
            TargetKind::Http => self
                .value
                .rsplit("://")
                .next()
                .unwrap_or(&self.value)
                .to_string(),
            _ => self.value.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ChannelConfig {
    pub enabled: bool,
    pub name: String,
    pub good_ms: f64,
    pub warn_ms: f64,
    pub targets: Vec<Target>,
    pub proxy: String,
    pub controller: String,
    pub controller_secret: String,
}

impl Default for ChannelConfig {
    fn default() -> Self {
        ChannelConfig {
            enabled: true,
            name: String::new(),
            good_ms: 300.0,
            warn_ms: 800.0,
            targets: Vec::new(),
            proxy: String::new(),
            controller: String::new(),
            controller_secret: String::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct BridgeConfig {
    pub enabled: bool,
    pub host: String,
    pub port: u16,
    pub token: String,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub path: PathBuf,
    pub interval: f64,
    pub timeout: f64,
    pub verify_ssl: bool,
    pub clash: ChannelConfig,
    pub vpn: ChannelConfig,
    pub direct: ChannelConfig,
    pub adapter_keywords: Vec<String>,
    pub process_names: Vec<String>,
    /// 是否允许用 ICMP 探测内网。部分公司会监控内网 ICMP，可关掉改用 TCP
    pub allow_icmp: bool,
    pub bridge: BridgeConfig,
}

impl Config {
    pub fn load(path: &Path) -> Config {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        let root = Json::parse(&text).unwrap_or(Json::Obj(Vec::new()));
        if text.trim().is_empty() {
            let defaults = Config::default();
            let _ = std::fs::write(path, defaults.to_json().to_string_pretty());
            return defaults;
        }
        Config::from_json(path, &root)
    }

    fn from_json(path: &Path, root: &Json) -> Config {
        let default = Config::default();
        let channels = |key: &str, fallback: &ChannelConfig| ChannelConfig {
            enabled: root.flag(&format!("{}.enabled", key), fallback.enabled),
            name: {
                let name = root.string(&format!("{}.name", key), "");
                if name.is_empty() {
                    fallback.name.clone()
                } else {
                    name
                }
            },
            good_ms: root.number(&format!("{}.good_ms", key), fallback.good_ms),
            warn_ms: root.number(&format!("{}.warn_ms", key), fallback.warn_ms),
            targets: parse_targets(root.path(&format!("{}.targets", key))),
            proxy: root.string(&format!("{}.proxy", key), &fallback.proxy),
            controller: root.string(&format!("{}.controller", key), &fallback.controller),
            controller_secret: root.string(
                &format!("{}.controller_secret", key),
                &fallback.controller_secret,
            ),
        };

        let mut config = Config {
            path: path.to_path_buf(),
            interval: root.number("interval_seconds", default.interval).max(1.0),
            timeout: root.number("timeout_seconds", default.timeout).max(0.5),
            verify_ssl: root.flag("verify_ssl", default.verify_ssl),
            clash: channels("clash", &default.clash),
            vpn: channels("vpn", &default.vpn),
            direct: channels("direct", &default.direct),
            adapter_keywords: {
                let list = root.string_list("vpn.adapter_keywords");
                if list.is_empty() {
                    default.adapter_keywords
                } else {
                    list
                }
            },
            process_names: {
                let list = root.string_list("vpn.process_names");
                if list.is_empty() {
                    default.process_names
                } else {
                    list
                }
            },
            allow_icmp: root.flag("vpn.allow_icmp", default.allow_icmp),
            bridge: BridgeConfig {
                enabled: root.flag("ai_bridge.enabled", default.bridge.enabled),
                host: root.string("ai_bridge.host", &default.bridge.host),
                port: root.number("ai_bridge.port", default.bridge.port as f64) as u16,
                token: root.string("ai_bridge.token", &default.bridge.token),
            },
        };
        if config.bridge.port == 0 {
            config.bridge.port = default.bridge.port;
        }
        if !config.vpn.enabled {
            config.vpn.enabled = default.vpn.enabled;
        }
        config
    }

    pub fn to_json(&self) -> Json {
        let channel = |cfg: &ChannelConfig| {
            Json::Obj(vec![
                ("enabled".into(), Json::Bool(cfg.enabled)),
                ("name".into(), Json::Str(cfg.name.clone())),
                (
                    "targets".into(),
                    Json::Arr(cfg.targets.iter().map(|t| Json::Str(t.value.clone())).collect()),
                ),
                ("good_ms".into(), Json::Num(cfg.good_ms)),
                ("warn_ms".into(), Json::Num(cfg.warn_ms)),
            ])
        };
        Json::Obj(vec![
            ("interval_seconds".into(), Json::Num(self.interval)),
            ("timeout_seconds".into(), Json::Num(self.timeout)),
            ("verify_ssl".into(), Json::Bool(self.verify_ssl)),
            (
                "clash".into(),
                Json::Obj(vec![
                    ("enabled".into(), Json::Bool(self.clash.enabled)),
                    ("name".into(), Json::Str(self.clash.name.clone())),
                    ("proxy".into(), Json::Str(self.clash.proxy.clone())),
                    ("controller".into(), Json::Str(self.clash.controller.clone())),
                    (
                        "controller_secret".into(),
                        Json::Str(self.clash.controller_secret.clone()),
                    ),
                    (
                        "targets".into(),
                        Json::Arr(
                            self.clash
                                .targets
                                .iter()
                                .map(|t| Json::Str(t.value.clone()))
                                .collect(),
                        ),
                    ),
                    ("good_ms".into(), Json::Num(self.clash.good_ms)),
                    ("warn_ms".into(), Json::Num(self.clash.warn_ms)),
                ]),
            ),
            (
                "vpn".into(),
                Json::Obj(vec![
                    ("enabled".into(), Json::Bool(self.vpn.enabled)),
                    ("name".into(), Json::Str(self.vpn.name.clone())),
                    (
                        "adapter_keywords".into(),
                        Json::Arr(
                            self.adapter_keywords
                                .iter()
                                .map(|k| Json::Str(k.clone()))
                                .collect(),
                        ),
                    ),
                    (
                        "process_names".into(),
                        Json::Arr(
                            self.process_names
                                .iter()
                                .map(|p| Json::Str(p.clone()))
                                .collect(),
                        ),
                    ),
                    ("targets".into(), Json::Arr(Vec::new())),
                    ("allow_icmp".into(), Json::Bool(self.allow_icmp)),
                    ("good_ms".into(), Json::Num(self.vpn.good_ms)),
                    ("warn_ms".into(), Json::Num(self.vpn.warn_ms)),
                ]),
            ),
            ("direct".into(), channel(&self.direct)),
            (
                "ai_bridge".into(),
                Json::Obj(vec![
                    ("enabled".into(), Json::Bool(self.bridge.enabled)),
                    ("host".into(), Json::Str(self.bridge.host.clone())),
                    ("port".into(), Json::Num(self.bridge.port as f64)),
                    ("token".into(), Json::Str(self.bridge.token.clone())),
                ]),
            ),
        ])
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            path: PathBuf::from("config.json"),
            interval: 5.0,
            timeout: 2.5,
            verify_ssl: true,
            clash: ChannelConfig {
                enabled: true,
                name: "外网 · Clash".into(),
                good_ms: 600.0,
                warn_ms: 1500.0,
                targets: vec![
                    Target::http("https://www.gstatic.com/generate_204", true),
                    Target::http("https://www.cloudflare.com/cdn-cgi/trace", true),
                ],
                proxy: "http://127.0.0.1:7890".into(),
                controller: "http://127.0.0.1:9090".into(),
                controller_secret: String::new(),
            },
            vpn: ChannelConfig {
                enabled: true,
                name: "内网 · OpenVPN".into(),
                good_ms: 150.0,
                warn_ms: 500.0,
                targets: Vec::new(),
                proxy: String::new(),
                controller: String::new(),
                controller_secret: String::new(),
            },
            direct: ChannelConfig {
                enabled: true,
                name: "国内 · 直连".into(),
                good_ms: 300.0,
                warn_ms: 800.0,
                targets: vec![
                    Target::http("https://www.baidu.com", true),
                    Target::http("https://www.qq.com", true),
                ],
                proxy: String::new(),
                controller: String::new(),
                controller_secret: String::new(),
            },
            adapter_keywords: ["TAP-Windows", "OpenVPN", "Wintun", "TUN"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            process_names: ["openvpn.exe", "openvpn-gui.exe"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            allow_icmp: true,
            bridge: BridgeConfig {
                enabled: true,
                host: "127.0.0.1".into(),
                port: 17800,
                token: String::new(),
            },
        }
    }
}

impl Target {
    pub fn http(value: &str, verify: bool) -> Target {
        Target {
            kind: TargetKind::Http,
            value: value.to_string(),
            name: String::new(),
            verify: if verify { None } else { Some(false) },
            port: None,
        }
    }

}

/// 从 "host:port" 里拆出主机与端口。
pub fn split_host_port(value: &str) -> Option<(String, u16)> {
    let (host, port) = value.rsplit_once(':')?;
    Some((host.to_string(), port.parse::<u16>().ok()?))
}

fn parse_targets(node: Option<&Json>) -> Vec<Target> {
    let mut result = Vec::new();
    let items = match node.and_then(|v| v.as_arr()) {
        Some(items) => items,
        None => return result,
    };
    for item in items {
        match item {
            Json::Str(text) => result.push(Target::http(text, true)),
            Json::Obj(_) => {
                let value = item.string("target", "");
                if value.is_empty() {
                    continue;
                }
                let kind = match item.string("type", "http").to_lowercase().as_str() {
                    "tcp" => TargetKind::Tcp,
                    "icmp" | "ping" => TargetKind::Icmp,
                    _ => TargetKind::Http,
                };
                let verify = item.path("verify").and_then(|v| v.as_bool());
                let port = split_host_port(&value)
                    .map(|(_, port)| port)
                    .filter(|_| kind == TargetKind::Tcp);
                result.push(Target {
                    kind,
                    value,
                    name: item.string("name", ""),
                    verify,
                    port,
                });
            }
            _ => {}
        }
    }
    result
}
