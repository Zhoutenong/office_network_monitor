//! WinHTTP / IP Helper / 注册表 的声明。

use super::*;
use std::ffi::c_void;

// ---------------------------------------------------------------- WinHTTP 常量
pub const ACCESS_TYPE_DEFAULT_PROXY: u32 = 0;
pub const ACCESS_TYPE_NO_PROXY: u32 = 1;
pub const ACCESS_TYPE_NAMED_PROXY: u32 = 3;
pub const FLAG_SECURE: u32 = 0x0080_0000;
pub const QUERY_STATUS_CODE: u32 = 19;
pub const QUERY_FLAG_NUMBER: u32 = 0x2000_0000;
pub const OPTION_SECURITY_FLAGS: u32 = 31;
pub const SECURITY_FLAG_IGNORE_UNKNOWN_CA: u32 = 0x0000_0100;
pub const SECURITY_FLAG_IGNORE_CERT_WRONG_USAGE: u32 = 0x0000_0200;
pub const SECURITY_FLAG_IGNORE_CERT_CN_INVALID: u32 = 0x0000_1000;
pub const SECURITY_FLAG_IGNORE_CERT_DATE_INVALID: u32 = 0x0000_2000;

// WinHTTP 错误码（用于给出和 Python 版一致的归因）
pub const ERROR_WINHTTP_CANNOT_CONNECT: u32 = 12029;
pub const ERROR_WINHTTP_NAME_NOT_RESOLVED: u32 = 12007;
pub const ERROR_WINHTTP_TIMEOUT: u32 = 12002;
pub const ERROR_WINHTTP_SECURE_FAILURE: u32 = 12175;
pub const ERROR_WINHTTP_CONNECTION_ERROR: u32 = 12030;
pub const ERROR_WINHTTP_INVALID_URL: u32 = 12181;
pub const ERROR_WINHTTP_UNRECOGNIZED_SCHEME: u32 = 12006;

#[link(name = "winhttp")]
extern "system" {
    pub fn WinHttpOpen(
        agent: *const u16,
        access_type: u32,
        proxy: *const u16,
        bypass: *const u16,
        flags: u32,
    ) -> HINTERNET;
    pub fn WinHttpConnect(
        session: HINTERNET,
        server: *const u16,
        port: u16,
        reserved: u32,
    ) -> HINTERNET;
    pub fn WinHttpOpenRequest(
        connect: HINTERNET,
        verb: *const u16,
        object: *const u16,
        version: *const u16,
        referrer: *const u16,
        accept_types: *const *const u16,
        flags: u32,
    ) -> HINTERNET;
    pub fn WinHttpSetTimeouts(
        handle: HINTERNET,
        resolve: i32,
        connect: i32,
        send: i32,
        receive: i32,
    ) -> BOOL;
    pub fn WinHttpSetOption(
        handle: HINTERNET,
        option: u32,
        buffer: *mut c_void,
        length: u32,
    ) -> BOOL;
    pub fn WinHttpSendRequest(
        request: HINTERNET,
        headers: *const u16,
        headers_length: u32,
        optional: *mut c_void,
        optional_length: u32,
        total_length: u32,
        context: usize,
    ) -> BOOL;
    pub fn WinHttpReceiveResponse(request: HINTERNET, reserved: *mut c_void) -> BOOL;
    pub fn WinHttpQueryHeaders(
        request: HINTERNET,
        info_level: u32,
        name: *const u16,
        buffer: *mut c_void,
        buffer_length: *mut u32,
        index: *mut u32,
    ) -> BOOL;
    pub fn WinHttpReadData(
        request: HINTERNET,
        buffer: *mut c_void,
        to_read: u32,
        read: *mut u32,
    ) -> BOOL;
    pub fn WinHttpCloseHandle(handle: HINTERNET) -> BOOL;
}

// ---------------------------------------------------------------- IP Helper
pub const GAA_FLAG_INCLUDE_GATEWAYS: u32 = 0x0080;
pub const AF_INET: u32 = 2;
pub const AF_UNSPEC: u32 = 0;
pub const IF_OPER_STATUS_UP: u32 = 1;
pub const TCP_TABLE_OWNER_PID_ALL: u32 = 5;
pub const MIB_TCP_STATE_ESTAB: u32 = 5;
pub const ERROR_INSUFFICIENT_BUFFER: u32 = 122;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct SOCKET_ADDRESS {
    pub lp_sockaddr: *mut c_void,
    pub i_sockaddr_length: i32,
}

impl Default for SOCKET_ADDRESS {
    fn default() -> Self {
        SOCKET_ADDRESS {
            lp_sockaddr: std::ptr::null_mut(),
            i_sockaddr_length: 0,
        }
    }
}

/// unicast / DNS / gateway 三类链表节点布局一致
#[repr(C)]
pub struct IP_ADAPTER_ADDRESS {
    pub length: u32,
    pub flags_or_ifindex: u32,
    pub next: *mut IP_ADAPTER_ADDRESS,
    pub address: SOCKET_ADDRESS,
}

#[repr(C)]
pub struct IP_ADAPTER_ADDRESSES_LH {
    pub length: u32,
    pub if_index: u32,
    pub next: *mut IP_ADAPTER_ADDRESSES_LH,
    pub adapter_name: *mut u8,
    pub first_unicast: *mut IP_ADAPTER_ADDRESS,
    pub first_anycast: *mut IP_ADAPTER_ADDRESS,
    pub first_multicast: *mut IP_ADAPTER_ADDRESS,
    pub first_dns_server: *mut IP_ADAPTER_ADDRESS,
    pub dns_suffix: *mut u16,
    pub description: *mut u16,
    pub friendly_name: *mut u16,
    pub physical_address: [u8; 8],
    pub physical_address_length: u32,
    pub flags: u32,
    pub mtu: u32,
    pub if_type: u32,
    pub oper_status: u32,
    pub ipv6_if_index: u32,
    pub zone_indices: [u32; 16],
    pub first_prefix: *mut c_void,
    pub transmit_link_speed: u64,
    pub receive_link_speed: u64,
    pub first_wins_server: *mut IP_ADAPTER_ADDRESS,
    pub first_gateway: *mut IP_ADAPTER_ADDRESS,
}

#[repr(C)]
pub struct IP_OPTION_INFORMATION {
    pub ttl: u8,
    pub tos: u8,
    pub flags: u8,
    pub options_size: u8,
    pub options_data: *mut u8,
}

#[repr(C)]
pub struct ICMP_ECHO_REPLY {
    pub address: u32,
    pub status: u32,
    pub round_trip_time: u32,
    pub data_size: u16,
    pub reserved: u16,
    pub data: *mut c_void,
    pub options: IP_OPTION_INFORMATION,
}

#[link(name = "iphlpapi")]
extern "system" {
    pub fn GetAdaptersAddresses(
        family: u32,
        flags: u32,
        reserved: *mut c_void,
        addresses: *mut IP_ADAPTER_ADDRESSES_LH,
        size: *mut u32,
    ) -> u32;
    pub fn IcmpCreateFile() -> HANDLE;
    pub fn IcmpSendEcho(
        handle: HANDLE,
        destination: u32,
        request_data: *mut c_void,
        request_size: u16,
        request_options: *mut IP_OPTION_INFORMATION,
        reply_buffer: *mut c_void,
        reply_size: u32,
        timeout: u32,
    ) -> u32;
    pub fn IcmpCloseHandle(handle: HANDLE) -> BOOL;
    pub fn GetExtendedTcpTable(
        table: *mut c_void,
        size: *mut u32,
        order: BOOL,
        family: u32,
        table_class: u32,
        reserved: u32,
    ) -> u32;
}

/// GetExtendedTcpTable 返回的一行（IPv4）
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MIB_TCPROW_OWNER_PID {
    pub state: u32,
    pub local_addr: u32,
    pub local_port: u32,
    pub remote_addr: u32,
    pub remote_port: u32,
    pub owning_pid: u32,
}

/// 端口在结构体里是网络字节序，且只占低 16 位
pub fn port_of(raw: u32) -> u16 {
    u16::from_be((raw & 0xFFFF) as u16)
}

// ---------------------------------------------------------------- 注册表
pub const HKEY_CURRENT_USER: HKEY = 0x8000_0001u32 as usize as HKEY;
pub const KEY_READ: u32 = 0x20019;
pub const REG_SZ: u32 = 1;
pub const REG_DWORD: u32 = 4;

#[link(name = "advapi32")]
extern "system" {
    pub fn RegOpenKeyExW(
        key: HKEY,
        sub_key: *const u16,
        options: u32,
        desired: u32,
        result: *mut HKEY,
    ) -> i32;
    pub fn RegQueryValueExW(
        key: HKEY,
        value_name: *const u16,
        reserved: *mut u32,
        value_type: *mut u32,
        data: *mut u8,
        data_size: *mut u32,
    ) -> i32;
    pub fn RegCloseKey(key: HKEY) -> i32;
}
