//! 把快照整理成大模型友好的结构化数据（JSON / Markdown），与 Python 版同构。

use crate::json::Json;
use crate::monitor::{Level, Snapshot};

fn level_icon(level: Level) -> &'static str {
    match level {
        Level::Good => "🟢",
        Level::Warn => "🟡",
        Level::Bad => "🔴",
        Level::Off => "⚪",
        Level::Unknown => "⚫",
    }
}

fn stats(history: &[Option<f64>]) -> Json {
    let values: Vec<f64> = history.iter().filter_map(|value| *value).collect();
    let mut fields: Vec<(String, Json)> = vec![
        ("samples".into(), Json::Num(history.len() as f64)),
        ("ok".into(), Json::Num(values.len() as f64)),
        (
            "failed".into(),
            Json::Num((history.len() - values.len()) as f64),
        ),
    ];
    if !values.is_empty() {
        let mut sorted = values.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let min = sorted[0];
        let max = sorted[sorted.len() - 1];
        let median = sorted[sorted.len() / 2];
        let mean = values.iter().sum::<f64>() / values.len() as f64;
        fields.push(("last_ms".into(), Json::Num(round1(*values.last().unwrap()))));
        fields.push(("min_ms".into(), Json::Num(round1(min))));
        fields.push(("max_ms".into(), Json::Num(round1(max))));
        fields.push(("median_ms".into(), Json::Num(round1(median))));
        fields.push(("mean_ms".into(), Json::Num(round1(mean))));
        fields.push(("jitter_ms".into(), Json::Num(round1(max - min))));
    }
    Json::Obj(fields)
}

fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

pub fn to_json(snapshot: &Snapshot) -> String {
    let mut channels = Vec::new();
    for channel in &snapshot.channels {
        channels.push((
            channel.key.to_string(),
            Json::Obj(vec![
                ("name".into(), Json::Str(channel.name.clone())),
                ("level".into(), Json::Str(channel.level.key().to_string())),
                ("level_text".into(), Json::Str(channel.level.text().to_string())),
                ("status".into(), Json::Str(channel.status.clone())),
                ("detail".into(), Json::Str(channel.detail.clone())),
                (
                    "last_latency_ms".into(),
                    channel
                        .latency_ms
                        .map(|value| Json::Num(round1(value)))
                        .unwrap_or(Json::Null),
                ),
                ("stats".into(), stats(&channel.history)),
                (
                    "history_ms".into(),
                    Json::Arr(
                        channel
                            .history
                            .iter()
                            .map(|value| match value {
                                Some(ms) => Json::Num(round1(*ms)),
                                None => Json::Null,
                            })
                            .collect(),
                    ),
                ),
            ]),
        ));
    }

    let environment = Json::Obj(vec![
        (
            "system_proxy".into(),
            Json::Obj(vec![
                ("enabled".into(), Json::Bool(snapshot.proxy_enabled)),
                ("server".into(), Json::Str(snapshot.proxy_server.clone())),
            ]),
        ),
        ("clash".into(), Json::Str(snapshot.clash_meta.clone())),
        (
            "context".into(),
            snapshot.extra.clone(),
        ),
    ]);

    Json::Obj(vec![
        ("generated_at".into(), Json::Str(snapshot.stamp.clone())),
        (
            "overall_level".into(),
            Json::Str(snapshot.overall().0.key().to_string()),
        ),
        (
            "overall_text".into(),
            Json::Str(snapshot.overall().1.clone()),
        ),
        ("channels".into(), Json::Obj(channels)),
        ("environment".into(), environment),
    ])
    .to_string_pretty()
}

pub fn to_markdown(snapshot: &Snapshot) -> String {
    let mut lines: Vec<String> = Vec::new();
    let interval = snapshot
        .extra
        .path("interval_seconds")
        .and_then(|value| value.as_f64())
        .unwrap_or(5.0);
    lines.push("# 办公网络状态报告".to_string());
    lines.push(String::new());
    lines.push(format!(
        "- 生成时间：{}（采样间隔 {} 秒，每条通道保留最近 {} 次）",
        snapshot.stamp,
        interval,
        snapshot
            .channels
            .iter()
            .map(|channel| channel.history.len())
            .max()
            .unwrap_or(0)
    ));
    lines.push(format!("- 总体状态：{}", snapshot.overall().1));
    lines.push(String::new());
    lines.push("## 通道状态".to_string());
    lines.push(String::new());
    lines.push("| 通道 | 状态 | 最近延迟 | 成功率 | 最小/中位/最大 | 抖动 | 说明 |".to_string());
    lines.push("| --- | --- | --- | --- | --- | --- | --- |".to_string());

    for channel in &snapshot.channels {
        let values: Vec<f64> = channel.history.iter().filter_map(|value| *value).collect();
        let (min, median, max, jitter) = if values.is_empty() {
            ("-".to_string(), "-".to_string(), "-".to_string(), "-".to_string())
        } else {
            let mut sorted = values.clone();
            sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            (
                format!("{}", round1(sorted[0])),
                format!("{}", round1(sorted[sorted.len() / 2])),
                format!("{}", round1(sorted[sorted.len() - 1])),
                format!("{}", round1(sorted[sorted.len() - 1] - sorted[0])),
            )
        };
        lines.push(format!(
            "| {} | {} {} | {} | {}/{} | {}/{}/{} | {} | {} |",
            channel.name,
            level_icon(channel.level),
            channel.status,
            channel
                .latency_ms
                .map(|value| format!("{:.0} ms", value))
                .unwrap_or_else(|| "-".to_string()),
            values.len(),
            channel.history.len(),
            min,
            median,
            max,
            jitter,
            channel.detail.replace('|', "/")
        ));
    }

    lines.push(String::new());
    lines.push("> 状态含义：🟢 正常 ｜ 🟡 偏慢（超过告警阈值）｜ 🔴 极慢或不通 ｜ ⚪ 未连接 / 未启用 / 未配置".to_string());
    lines.push(String::new());
    lines.push("## 本机环境".to_string());
    lines.push(String::new());
    lines.push(format!(
        "- 系统代理：{}{}",
        if snapshot.proxy_enabled { "开启" } else { "关闭" },
        if snapshot.proxy_server.is_empty() {
            String::new()
        } else {
            format!("（{}）", snapshot.proxy_server)
        }
    ));
    if !snapshot.clash_meta.is_empty() {
        lines.push(format!("- Clash：{}", snapshot.clash_meta));
    }
    if let Some(vpn) = snapshot.extra.path("vpn") {
        let adapter = vpn.string("adapter", "");
        let ipv4 = join_strings(vpn.path("ipv4"));
        let gateway = join_strings(vpn.path("gateway"));
        let dns = join_strings(vpn.path("dns"));
        lines.push(format!(
            "- VPN 网卡：{}（本机 {}，网关 {}，DNS {}）",
            if adapter.is_empty() { "已连接" } else { &adapter },
            if ipv4.is_empty() { "-" } else { &ipv4 },
            if gateway.is_empty() { "-" } else { &gateway },
            if dns.is_empty() { "-" } else { &dns },
        ));
    }
    if let Some(targets) = snapshot.extra.path("targets") {
        for key in ["clash", "vpn", "direct"] {
            let list = join_strings(targets.path(key));
            if !list.is_empty() {
                lines.push(format!("- {} 探测目标：{}", key, list));
            }
        }
    }
    if let Some(thresholds) = snapshot.extra.path("thresholds") {
        let mut parts = Vec::new();
        for key in ["clash", "vpn", "direct"] {
            if let Some(item) = thresholds.path(key) {
                parts.push(format!(
                    "{} 正常≤{:.0}ms、偏慢≤{:.0}ms",
                    key,
                    item.number("good", 0.0),
                    item.number("warn", 0.0)
                ));
            }
        }
        if !parts.is_empty() {
            lines.push(format!("- 判定阈值：{}", parts.join("；")));
        }
    }

    lines.push(String::new());
    lines.push("## 原始采样（ms，null 表示该次失败）".to_string());
    lines.push(String::new());
    for channel in &snapshot.channels {
        let values: Vec<String> = channel
            .history
            .iter()
            .map(|value| match value {
                Some(ms) => format!("{:.0}", ms),
                None => "null".to_string(),
            })
            .collect();
        lines.push(format!(
            "- {}：[{}]",
            channel.name,
            values.join(", ")
        ));
    }
    lines.push(String::new());
    lines.join("\n")
}

fn join_strings(node: Option<&Json>) -> String {
    node.and_then(|value| value.as_arr())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(|text| text.to_string()))
                .collect::<Vec<String>>()
                .join(", ")
        })
        .unwrap_or_default()
}
