//! 探测层：HTTP(WinHTTP) / TCP / ICMP，以及网卡、TCP 对端、系统代理等本机状态。
//!
//! 与 Python 版保持同一套语义：
//! - HTTP 走 WinHTTP，延迟口径是「发出请求 → 收到响应头」
//! - 连接按 host:port 复用，避免每轮重做 TLS 握手
//! - 证书校验收口用系统证书库（不需要自带 CA 包）

use std::ffi::c_void;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

use crate::ffi::net;
use crate::ffi::{self, HANDLE, HINTERNET};

#[derive(Debug, Clone)]
pub struct ProbeResult {
    pub ok: bool,
    pub latency_ms: Option<f64>,
    pub detail: String,
    /// 代理本身连不上（用于区分「Clash 没开」和「节点不通」）
    pub proxy_down: bool,
    /// 仅在需要读取响应体时填充（例如读 Clash 控制器）
    pub body: Option<String>,
}

impl ProbeResult {
    pub fn ok(latency_ms: f64, detail: impl Into<String>) -> ProbeResult {
        ProbeResult {
            ok: true,
            latency_ms: Some(latency_ms),
            detail: detail.into(),
            proxy_down: false,
            body: None,
        }
    }

    pub fn fail(detail: impl Into<String>) -> ProbeResult {
        ProbeResult {
            ok: false,
            latency_ms: None,
            detail: detail.into(),
            proxy_down: false,
            body: None,
        }
    }
}

/// 把 WinHTTP 的错误码翻译成简短中文说明。
fn winhttp_error(code: u32) -> (String, bool) {
    match code {
        net::ERROR_WINHTTP_CANNOT_CONNECT => ("代理/主机连不上".to_string(), true),
        net::ERROR_WINHTTP_NAME_NOT_RESOLVED => ("域名解析失败".to_string(), false),
        net::ERROR_WINHTTP_TIMEOUT => ("超时".to_string(), false),
        net::ERROR_WINHTTP_SECURE_FAILURE => ("证书错误".to_string(), false),
        net::ERROR_WINHTTP_CONNECTION_ERROR => ("连接被中断".to_string(), false),
        net::ERROR_WINHTTP_INVALID_URL | net::ERROR_WINHTTP_UNRECOGNIZED_SCHEME => {
            ("地址无效".to_string(), false)
        }
        other => (format!("连接失败(错误 {})", other), false),
    }
}

pub struct HttpSession {
    handle: HINTERNET,
    connections: Vec<(String, u16, HINTERNET)>,
}

impl HttpSession {
    /// `proxy` 形如 "http://127.0.0.1:7890"；为空表示直连。
    pub fn new(proxy: &str) -> Option<HttpSession> {
        let agent = ffi::wide("office-network-monitor/1.0");
        let cleaned = proxy
            .trim()
            .trim_start_matches("http://")
            .trim_start_matches("https://")
            .trim_end_matches('/')
            .to_string();
        let (access, proxy_wide) = if cleaned.is_empty() {
            (net::ACCESS_TYPE_NO_PROXY, Vec::new())
        } else {
            (net::ACCESS_TYPE_NAMED_PROXY, ffi::wide(&cleaned))
        };
        let proxy_ptr = if proxy_wide.is_empty() {
            std::ptr::null()
        } else {
            proxy_wide.as_ptr()
        };
        let handle = unsafe { net::WinHttpOpen(agent.as_ptr(), access, proxy_ptr, std::ptr::null(), 0) };
        if handle.is_null() {
            return None;
        }
        Some(HttpSession {
            handle,
            connections: Vec::new(),
        })
    }

    fn connect(&mut self, host: &str, port: u16) -> HINTERNET {
        if let Some((_, _, handle)) = self
            .connections
            .iter()
            .find(|(h, p, _)| h == host && *p == port)
        {
            return *handle;
        }
        let host_wide = ffi::wide(host);
        let handle = unsafe { net::WinHttpConnect(self.handle, host_wide.as_ptr(), port, 0) };
        if handle.is_null() {
            return std::ptr::null_mut();
        }
        if self.connections.len() >= 4 {
            let (_, _, old) = self.connections.remove(0);
            unsafe { net::WinHttpCloseHandle(old) };
        }
        self.connections.push((host.to_string(), port, handle));
        handle
    }

    /// 发一次 GET，返回「到收到响应头」的耗时。
    pub fn get(&mut self, url: &str, timeout: Duration, verify: bool) -> ProbeResult {
        self.request(url, timeout, verify, false)
    }

    /// 与 get 相同，但把响应体也读回来（用于读取 Clash 控制器的 JSON）。
    pub fn get_text(&mut self, url: &str, timeout: Duration) -> ProbeResult {
        self.request(url, timeout, true, true)
    }

    fn request(&mut self, url: &str, timeout: Duration, verify: bool, collect: bool) -> ProbeResult {
        let parsed = match parse_url(url) {
            Some(value) => value,
            None => return ProbeResult::fail("地址无效"),
        };
        let connect = self.connect(&parsed.host, parsed.port);
        if connect.is_null() {
            return ProbeResult::fail("连接建立失败");
        }

        let verb = ffi::wide("GET");
        let object = ffi::wide(&parsed.path);
        // WINHTTP_FLAG_ESCAPE_DISABLE：路径里已有转义，别二次编码
        let flags = if parsed.secure { net::FLAG_SECURE } else { 0 } | 0x0000_0100;
        let request = unsafe {
            net::WinHttpOpenRequest(
                connect,
                verb.as_ptr(),
                object.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                flags,
            )
        };
        if request.is_null() {
            return ProbeResult::fail("请求创建失败");
        }
        self.send(request, timeout, verify, parsed.host, collect)
    }

    fn send(
        &mut self,
        request: HINTERNET,
        timeout: Duration,
        verify: bool,
        host: String,
        collect: bool,
    ) -> ProbeResult {
        let ms = (timeout.as_millis().max(500) as i32).min(60_000);
        unsafe {
            net::WinHttpSetTimeouts(request, ms, ms, ms, ms);
            if !verify {
                let flags = net::SECURITY_FLAG_IGNORE_UNKNOWN_CA
                    | net::SECURITY_FLAG_IGNORE_CERT_CN_INVALID
                    | net::SECURITY_FLAG_IGNORE_CERT_DATE_INVALID
                    | net::SECURITY_FLAG_IGNORE_CERT_WRONG_USAGE;
                net::WinHttpSetOption(
                    request,
                    net::OPTION_SECURITY_FLAGS,
                    &flags as *const u32 as *mut c_void,
                    4,
                );
            }
        }

        let started = Instant::now();
        let sent = unsafe {
            net::WinHttpSendRequest(
                request,
                std::ptr::null(),
                0,
                std::ptr::null_mut(),
                0,
                0,
                0,
            )
        };
        if sent == 0 {
            let (detail, proxy_down) = winhttp_error(ffi::last_error());
            unsafe { net::WinHttpCloseHandle(request) };
            let mut result = ProbeResult::fail(detail);
            result.proxy_down = proxy_down;
            return result;
        }
        let received = unsafe { net::WinHttpReceiveResponse(request, std::ptr::null_mut()) };
        let latency = started.elapsed().as_secs_f64() * 1000.0;
        if received == 0 {
            let (detail, proxy_down) = winhttp_error(ffi::last_error());
            unsafe { net::WinHttpCloseHandle(request) };
            let mut result = ProbeResult::fail(detail);
            result.proxy_down = proxy_down;
            return result;
        }

        let mut status: u32 = 0;
        let mut length = std::mem::size_of::<u32>() as u32;
        let ok = unsafe {
            net::WinHttpQueryHeaders(
                request,
                net::QUERY_STATUS_CODE | net::QUERY_FLAG_NUMBER,
                std::ptr::null(),
                &mut status as *mut u32 as *mut c_void,
                &mut length,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            status = 0;
        }
        let body = if collect {
            Some(read_body_text(request))
        } else {
            drain_body(request);
            None
        };
        unsafe { net::WinHttpCloseHandle(request) };

        let mut result = if (200..400).contains(&status) {
            ProbeResult::ok(latency, format!("HTTP {}", status))
        } else if status == 0 {
            ProbeResult::fail(format!("响应异常 ({})", host))
        } else {
            ProbeResult {
                ok: false,
                latency_ms: Some(latency),
                detail: format!("HTTP {}", status),
                proxy_down: false,
                body: None,
            }
        };
        result.body = body;
        result
    }
}

// WinHTTP 句柄本质上是不透明的整数句柄，同一时刻只被一个线程使用，
// 因此可以安全地随 &mut HttpSession 在线程间移动（探测时每条通道各用一次）。
unsafe impl Send for HttpSession {}

impl Drop for HttpSession {
    fn drop(&mut self) {
        for (_, _, handle) in self.connections.drain(..) {
            unsafe { net::WinHttpCloseHandle(handle) };
        }
        if !self.handle.is_null() {
            unsafe { net::WinHttpCloseHandle(self.handle) };
        }
    }
}

/// 把响应体读掉：连接才会被放回连接池复用。
fn drain_body(request: HINTERNET) {
    let mut buffer = vec![0u8; 16 * 1024];
    let mut total = 0usize;
    loop {
        let mut read: u32 = 0;
        let ok = unsafe {
            net::WinHttpReadData(
                request,
                buffer.as_mut_ptr() as *mut c_void,
                buffer.len() as u32,
                &mut read,
            )
        };
        if ok == 0 || read == 0 {
            break;
        }
        total += read as usize;
        if total > 512 * 1024 {
            break;
        }
    }
}

/// 读取响应体（上限 64KB），用于 Clash 控制器这类小 JSON。
fn read_body_text(request: HINTERNET) -> String {
    let mut buffer = vec![0u8; 16 * 1024];
    let mut collected: Vec<u8> = Vec::new();
    loop {
        let mut read: u32 = 0;
        let ok = unsafe {
            net::WinHttpReadData(
                request,
                buffer.as_mut_ptr() as *mut c_void,
                buffer.len() as u32,
                &mut read,
            )
        };
        if ok == 0 || read == 0 {
            break;
        }
        collected.extend_from_slice(&buffer[..read as usize]);
        if collected.len() > 64 * 1024 {
            break;
        }
    }
    String::from_utf8_lossy(&collected).to_string()
}

pub struct ParsedUrl {
    pub secure: bool,
    pub host: String,
    pub port: u16,
    pub path: String,
}

pub fn parse_url(url: &str) -> Option<ParsedUrl> {
    let (secure, rest) = if let Some(rest) = url.strip_prefix("https://") {
        (true, rest)
    } else if let Some(rest) = url.strip_prefix("http://") {
        (false, rest)
    } else {
        return None;
    };
    let (authority, path) = match rest.find('/') {
        Some(index) => (&rest[..index], &rest[index..]),
        None => (rest, "/"),
    };
    if authority.is_empty() {
        return None;
    }
    let (host, port) = match authority.rfind(':') {
        Some(index) => {
            let port = authority[index + 1..].parse::<u16>().ok()?;
            (&authority[..index], port)
        }
        None => (authority, if secure { 443 } else { 80 }),
    };
    Some(ParsedUrl {
        secure,
        host: host.to_string(),
        port,
        path: if path.is_empty() { "/".to_string() } else { path.to_string() },
    })
}

/// 提取 URL 的域名，用于界面显示。
pub fn host_of(url: &str) -> String {
    parse_url(url).map(|p| p.host).unwrap_or_else(|| url.to_string())
}

// ------------------------------------------------------------------ TCP
pub fn tcp_probe(host: &str, port: u16, timeout: Duration) -> ProbeResult {
    let started = Instant::now();
    let address = match (host, port).to_socket_addrs() {
        Ok(mut iter) => iter.next(),
        Err(_) => return ProbeResult::fail("域名解析失败"),
    };
    let address = match address {
        Some(value) => value,
        None => return ProbeResult::fail("域名解析失败"),
    };
    match TcpStream::connect_timeout(&address, timeout) {
        Ok(_stream) => ProbeResult::ok(
            started.elapsed().as_secs_f64() * 1000.0,
            "TCP 已连接",
        ),
        Err(_) => ProbeResult::fail("连接超时"),
    }
}

// ------------------------------------------------------------------ ICMP
pub fn icmp_probe(host: &str, timeout: Duration) -> ProbeResult {
    let address = match host.parse::<std::net::Ipv4Addr>() {
        Ok(value) => value,
        Err(_) => match (host, 0u16).to_socket_addrs().ok().and_then(|mut it| it.next()) {
            Some(std::net::SocketAddr::V4(value)) => *value.ip(),
            _ => return ProbeResult::fail("域名解析失败"),
        },
    };
    let handle = unsafe { net::IcmpCreateFile() };
    if handle.is_null() || handle == (-1isize as HANDLE) {
        return ProbeResult::fail("ICMP 不可用");
    }
    // 载荷刻意与系统 ping 的默认内容一致（32 字节 a..w 循环）：
    // 固定长度 + 固定内容的 ICMP 是最容易被指纹识别成"信标"的特征
    let request: &[u8; 32] = b"abcdefghijklmnopqrstuvwabcdefghi";
    let mut reply = vec![0u8; 160];
    let started = Instant::now();
    let count = unsafe {
        net::IcmpSendEcho(
            handle,
            u32::from_ne_bytes(address.octets()),
            request.as_ptr() as *mut c_void,
            request.len() as u16,
            std::ptr::null_mut(),
            reply.as_mut_ptr() as *mut c_void,
            reply.len() as u32,
            timeout.as_millis().max(200) as u32,
        )
    };
    let elapsed = started.elapsed().as_secs_f64() * 1000.0;
    unsafe { net::IcmpCloseHandle(handle) };

    if count == 0 {
        return ProbeResult::fail("ping 无响应");
    }
    let echo = unsafe { &*(reply.as_ptr() as *const net::ICMP_ECHO_REPLY) };
    if echo.status != 0 {
        return ProbeResult::fail("不可达");
    }
    let rtt = if echo.round_trip_time == 0 {
        elapsed
    } else {
        echo.round_trip_time as f64
    };
    ProbeResult::ok(rtt, "ICMP 可达")
}

// ------------------------------------------------------------------ 网卡
#[derive(Debug, Clone, Default)]
pub struct AdapterInfo {
    pub name: String,
    pub description: String,
    pub up: bool,
    pub ipv4: Vec<String>,
    pub gateway: Vec<String>,
    pub dns: Vec<String>,
}

fn sockaddr_ip(address: net::SOCKET_ADDRESS) -> Option<String> {
    if address.lp_sockaddr.is_null() || address.i_sockaddr_length < 8 {
        return None;
    }
    let bytes = unsafe {
        std::slice::from_raw_parts(address.lp_sockaddr as *const u8, address.i_sockaddr_length as usize)
    };
    let family = u16::from_ne_bytes([bytes[0], bytes[1]]);
    if family != net::AF_INET as u16 {
        return None;
    }
    Some(format!("{}.{}.{}.{}", bytes[4], bytes[5], bytes[6], bytes[7]))
}

unsafe fn walk_addresses(mut node: *mut net::IP_ADAPTER_ADDRESS) -> Vec<String> {
    let mut result = Vec::new();
    while !node.is_null() && result.len() < 8 {
        let current = &*node;
        if let Some(ip) = sockaddr_ip(current.address) {
            result.push(ip);
        }
        node = current.next;
    }
    result
}

pub fn adapters() -> Vec<AdapterInfo> {
    let mut size: u32 = 16 * 1024;
    let mut buffer = vec![0u8; size as usize];
    let result = unsafe {
        net::GetAdaptersAddresses(
            net::AF_UNSPEC,
            net::GAA_FLAG_INCLUDE_GATEWAYS,
            std::ptr::null_mut(),
            buffer.as_mut_ptr() as *mut net::IP_ADAPTER_ADDRESSES_LH,
            &mut size,
        )
    };
    if result == net::ERROR_INSUFFICIENT_BUFFER {
        buffer = vec![0u8; size as usize];
        let retry = unsafe {
            net::GetAdaptersAddresses(
                net::AF_UNSPEC,
                net::GAA_FLAG_INCLUDE_GATEWAYS,
                std::ptr::null_mut(),
                buffer.as_mut_ptr() as *mut net::IP_ADAPTER_ADDRESSES_LH,
                &mut size,
            )
        };
        if retry != 0 {
            return Vec::new();
        }
    } else if result != 0 {
        return Vec::new();
    }

    let mut list = Vec::new();
    let mut node = buffer.as_ptr() as *mut net::IP_ADAPTER_ADDRESSES_LH;
    while !node.is_null() {
        let adapter = unsafe { &*node };
        let ipv4 = unsafe { walk_addresses(adapter.first_unicast) }
            .into_iter()
            .filter(|ip| !ip.starts_with("169.254."))
            .collect();
        list.push(AdapterInfo {
            name: unsafe { wide_string(adapter.friendly_name) },
            description: unsafe { wide_string(adapter.description) },
            up: adapter.oper_status == net::IF_OPER_STATUS_UP,
            ipv4,
            gateway: unsafe { walk_addresses(adapter.first_gateway) },
            dns: unsafe { walk_addresses(adapter.first_dns_server) },
        });
        node = adapter.next;
    }
    list
}

unsafe fn wide_string(pointer: *mut u16) -> String {
    if pointer.is_null() {
        return String::new();
    }
    let mut length = 0usize;
    while *pointer.add(length) != 0 {
        length += 1;
        if length > 512 {
            break;
        }
    }
    String::from_utf16_lossy(std::slice::from_raw_parts(pointer, length))
}

// ------------------------------------------------------------------ TCP 对端
/// 找出本机这些地址上「已建立」连接的对端，即正在使用的内网服务器。
pub fn tcp_peers(local_ips: &[String], limit: usize) -> Vec<(String, u16)> {
    if local_ips.is_empty() {
        return Vec::new();
    }
    let mut size: u32 = 0;
    unsafe {
        net::GetExtendedTcpTable(
            std::ptr::null_mut(),
            &mut size,
            0,
            net::AF_INET,
            net::TCP_TABLE_OWNER_PID_ALL,
            0,
        )
    };
    if size == 0 {
        return Vec::new();
    }
    let mut buffer = vec![0u8; size as usize];
    let result = unsafe {
        net::GetExtendedTcpTable(
            buffer.as_mut_ptr() as *mut c_void,
            &mut size,
            0,
            net::AF_INET,
            net::TCP_TABLE_OWNER_PID_ALL,
            0,
        )
    };
    if result != 0 {
        return Vec::new();
    }

    let count = u32::from_ne_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]) as usize;
    let header = std::mem::size_of::<u32>();
    let row_size = std::mem::size_of::<net::MIB_TCPROW_OWNER_PID>();
    let mut peers: Vec<(String, u16)> = Vec::new();
    for index in 0..count {
        let offset = header + index * row_size;
        if offset + row_size > buffer.len() {
            break;
        }
        let row = unsafe { &*(buffer.as_ptr().add(offset) as *const net::MIB_TCPROW_OWNER_PID) };
        if row.state != net::MIB_TCP_STATE_ESTAB {
            continue;
        }
        let local = ffi::u32_to_ipv4(row.local_addr);
        if !local_ips.iter().any(|ip| ip == &local) {
            continue;
        }
        let remote_ip = ffi::u32_to_ipv4(row.remote_addr);
        let remote_port = net::port_of(row.remote_port);
        if remote_port == 0 || remote_ip.starts_with("127.") || remote_ip == "0.0.0.0" {
            continue;
        }
        let peer = (remote_ip, remote_port);
        if !peers.contains(&peer) {
            peers.push(peer);
        }
        if peers.len() >= limit {
            break;
        }
    }
    peers
}

// ------------------------------------------------------------------ 系统代理
pub fn system_proxy() -> (bool, String) {
    use crate::ffi::net as regi;
    let sub_key = ffi::wide(r"Software\Microsoft\Windows\CurrentVersion\Internet Settings");
    let mut key: ffi::HKEY = std::ptr::null_mut();
    let opened = unsafe {
        regi::RegOpenKeyExW(
            regi::HKEY_CURRENT_USER,
            sub_key.as_ptr(),
            0,
            regi::KEY_READ,
            &mut key,
        )
    };
    if opened != 0 {
        return (false, String::new());
    }

    let mut enabled = false;
    let mut server = String::new();
    let enable_name = ffi::wide("ProxyEnable");
    let mut value_type: u32 = 0;
    let mut data: u32 = 0;
    let mut size = std::mem::size_of::<u32>() as u32;
    let got = unsafe {
        regi::RegQueryValueExW(
            key,
            enable_name.as_ptr(),
            std::ptr::null_mut(),
            &mut value_type,
            &mut data as *mut u32 as *mut u8,
            &mut size,
        )
    };
    if got == 0 {
        enabled = data != 0;
    }

    let server_name = ffi::wide("ProxyServer");
    let mut buffer = vec![0u8; 512];
    let mut size = buffer.len() as u32;
    let got = unsafe {
        regi::RegQueryValueExW(
            key,
            server_name.as_ptr(),
            std::ptr::null_mut(),
            &mut value_type,
            buffer.as_mut_ptr(),
            &mut size,
        )
    };
    if got == 0 && value_type == regi::REG_SZ {
        let units: Vec<u16> = buffer[..size as usize]
            .chunks_exact(2)
            .map(|pair| u16::from_ne_bytes([pair[0], pair[1]]))
            .take_while(|&unit| unit != 0)
            .collect();
        server = String::from_utf16_lossy(&units);
    }
    unsafe { regi::RegCloseKey(key) };
    (enabled, server)
}

// ------------------------------------------------------------------ 进程
pub fn running_processes(names: &[String]) -> Vec<String> {
    if names.is_empty() {
        return Vec::new();
    }
    let mut running = Vec::new();
    for name in names {
        if crate::process::is_running(name) {
            running.push(name.clone());
        }
    }
    running
}
