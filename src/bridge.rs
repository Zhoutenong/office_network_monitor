//! 只监听本机回环地址的只读 HTTP 接口，供脚本 / AI Agent 拉取当前网络状态。
//!
//! 用标准库的 TcpListener 手写，不引入 HTTP 库：
//! - GET /            简单说明页
//! - GET /status      当前状态 JSON
//! - GET /report      当前状态 Markdown 报告
//! 若配置了 token，则要求带上 ?token=xxx 或请求头 X-Token。

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;

use crate::report;
use crate::tray::Runtime;

const INDEX_HTML: &str = r#"<!doctype html>
<html lang="zh-CN"><head><meta charset="utf-8"><title>网络状态接口</title>
<style>
body{background:#151a21;color:#e8edf5;font:14px/1.7 "Microsoft YaHei UI",sans-serif;padding:32px}
a{color:#4f8cff;text-decoration:none} code{background:#1b2130;padding:2px 6px;border-radius:4px}
</style></head><body>
<h2>网络状态监测接口</h2>
<ul>
<li><a href="/status">/status</a> — 当前状态 JSON</li>
<li><a href="/report">/report</a> — Markdown 报告（可直接发给大模型）</li>
</ul>
<p>把 <code>{url}/report</code> 的内容贴给大模型，即可让它分析当前网络状况。</p>
</body></html>
"#;

/// 启动接口线程；返回对外展示的地址（未启用或端口被占用时返回 None）。
pub fn start(runtime: Arc<Runtime>) -> Option<String> {
    let (enabled, host, port, token) = {
        let engine = runtime.engine.lock().ok()?;
        let bridge = &engine.config.bridge;
        (
            bridge.enabled,
            bridge.host.clone(),
            bridge.port,
            bridge.token.clone(),
        )
    };
    if !enabled {
        return None;
    }
    let address = format!("{}:{}", host, port);
    let listener = match TcpListener::bind(&address) {
        Ok(value) => value,
        Err(error) => {
            crate::log::warn(&format!("状态接口启动失败（{}）：{}", address, error));
            return None;
        }
    };

    std::thread::spawn(move || {
        for incoming in listener.incoming() {
            match incoming {
                Ok(stream) => {
                    let client_runtime = Arc::clone(&runtime);
                    let client_token = token.clone();
                    std::thread::spawn(move || handle(stream, client_runtime, client_token));
                }
                Err(_) => break,
            }
        }
    });
    crate::log::write("INFO", &format!("状态接口已启动：http://{}/status", address));
    Some(format!("http://{}", address))
}

fn handle(mut stream: TcpStream, runtime: Arc<Runtime>, token: String) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
    let mut buffer = [0u8; 4096];
    let size = match stream.read(&mut buffer) {
        Ok(value) => value,
        Err(_) => return,
    };
    let request = String::from_utf8_lossy(&buffer[..size]).to_string();
    let first_line = request.lines().next().unwrap_or_default().to_string();
    let mut parts = first_line.split_whitespace();
    let _method = parts.next().unwrap_or("");
    let target = parts.next().unwrap_or("/");
    let (path, query) = match target.split_once('?') {
        Some((path, query)) => (path, query.to_string()),
        None => (target, String::new()),
    };

    if !token.is_empty() && !authorized(&request, &query, &token) {
        respond(
            &mut stream,
            401,
            "application/json; charset=utf-8",
            "{\"error\":\"unauthorized\",\"hint\":\"需要 ?token= 或 X-Token 请求头\"}",
        );
        return;
    }

    let snapshot = runtime
        .snapshot
        .lock()
        .ok()
        .and_then(|guard| guard.clone());

    match path {
        "/status" | "/status.json" => match snapshot {
            Some(value) => respond(
                &mut stream,
                200,
                "application/json; charset=utf-8",
                &report::to_json(&value),
            ),
            None => respond(
                &mut stream,
                503,
                "application/json; charset=utf-8",
                "{\"status\":\"warming_up\",\"message\":\"尚未完成首轮探测\"}",
            ),
        },
        "/report" | "/report.md" => match snapshot {
            Some(value) => respond(
                &mut stream,
                200,
                "text/markdown; charset=utf-8",
                &report::to_markdown(&value),
            ),
            None => respond(
                &mut stream,
                503,
                "text/plain; charset=utf-8",
                "尚未完成首轮探测",
            ),
        },
        "/" | "/index.html" => {
            let host = header(&request, "host").unwrap_or_else(|| "127.0.0.1".to_string());
            let html = INDEX_HTML.replace("{url}", &format!("http://{}", host));
            respond(&mut stream, 200, "text/html; charset=utf-8", &html)
        }
        _ => respond(
            &mut stream,
            404,
            "application/json; charset=utf-8",
            "{\"error\":\"not_found\",\"paths\":[\"/status\",\"/report\"]}",
        ),
    }
}

fn header(request: &str, name: &str) -> Option<String> {
    let wanted = format!("{}:", name.to_lowercase());
    request.lines().find_map(|line| {
        let lower = line.to_lowercase();
        if lower.starts_with(&wanted) {
            Some(line[line.find(':')? + 1..].trim().to_string())
        } else {
            None
        }
    })
}

fn authorized(request: &str, query: &str, token: &str) -> bool {
    if header(request, "x-token").as_deref() == Some(token) {
        return true;
    }
    query
        .split('&')
        .any(|pair| pair == format!("token={}", token))
}

fn respond(stream: &mut TcpStream, code: u16, content_type: &str, body: &str) {
    let bytes = body.as_bytes();
    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n",
        code,
        reason(code),
        content_type,
        bytes.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(bytes);
    let _ = stream.flush();
}

fn reason(code: u16) -> &'static str {
    match code {
        200 => "OK",
        401 => "Unauthorized",
        404 => "Not Found",
        503 => "Service Unavailable",
        _ => "OK",
    }
}
