//! 来源进程识别：将代理连接归属到发起进程（仅 Windows）。
//!
//! 原理：
//! 1. 代理 accept 时拿到（客户端 IP, 客户端端口）。
//! 2. 查询系统 TCP 表（GetExtendedTcpTable + TCP_TABLE_OWNER_PID_ALL），
//!    匹配（本地端口 = 代理端口, 远端地址/端口 = 客户端）的行，得到 owning PID。
//! 3. 用 ToolHelp 枚举进程快照，PID → 进程名（带进程表缓存）。
//!
//! 手机等远程设备连入的连接 owning_pid 为 0，解析不到进程，前端归为设备 IP。

use std::net::Ipv4Addr;
use std::sync::Mutex;

/// PID → 进程名缓存（避免每次连接都枚举进程表）。
static PID_NAME_CACHE: Mutex<Option<std::collections::HashMap<u32, String>>> =
    Mutex::new(None);

/// 从 TCP 表原始字节中查找匹配行，返回 owning PID。
///
/// 纯函数（便于测试）：`buf` 为 GetExtendedTcpTable 返回的内存，
/// 前 4 字节为条目数（小端），其后每 24 字节一条 MIB_TCPROW_OWNER_PID：
///   dwState(u32 LE) dwLocalAddr(u32 BE) dwLocalPort(u16 BE + 2 填充)
///   dwRemoteAddr(u32 BE) dwRemotePort(u16 BE + 2 填充) dwOwningPid(u32 LE)
/// 地址/端口按网络字节序（大端）存储。
pub fn parse_tcp_table(
    buf: &[u8],
    proxy_port: u16,
    client_ip: Ipv4Addr,
    client_port: u16,
) -> Option<u32> {
    if buf.len() < 4 {
        return None;
    }
    let num = u32::from_le_bytes(buf[0..4].try_into().unwrap_or([0; 4])) as usize;
    let row_len = 24usize;
    let client_ip_u32 = u32::from_be_bytes(client_ip.octets());

    for i in 0..num {
        let start = 4 + i * row_len;
        if start + row_len > buf.len() {
            break;
        }
        let row = &buf[start..start + row_len];
        let dw_local_addr = u32::from_be_bytes(row[4..8].try_into().unwrap());
        let dw_local_port = u16::from_be_bytes(row[8..10].try_into().unwrap());
        let dw_remote_addr = u32::from_be_bytes(row[12..16].try_into().unwrap());
        let dw_remote_port = u16::from_be_bytes(row[16..18].try_into().unwrap());
        let owning_pid = u32::from_le_bytes(row[20..24].try_into().unwrap());

        let _ = (dw_local_addr, dw_remote_port); // dwLocalAddr 可能为 0.0.0.0（绑定通配）

        if dw_local_port == proxy_port
            && dw_remote_port == client_port
            && dw_remote_addr == client_ip_u32
        {
            return Some(owning_pid);
        }
    }
    None
}

/// 枚举进程快照构建 PID→进程名 映射（Windows ToolHelp）。
#[cfg(windows)]
fn build_pid_name_map() -> std::collections::HashMap<u32, String> {
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    let mut map = std::collections::HashMap::new();
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
            return map;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        if Process32FirstW(snapshot, &mut entry) != 0 {
            loop {
                let pid = entry.th32ProcessID;
                let end = entry
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(entry.szExeFile.len());
                let name = String::from_utf16_lossy(&entry.szExeFile[..end]);
                map.insert(pid, name);
                if Process32NextW(snapshot, &mut entry) == 0 {
                    break;
                }
            }
        }
        windows_sys::Win32::Foundation::CloseHandle(snapshot);
    }
    map
}

/// PID → 进程名（带缓存；非 Windows 返回 None）。
pub fn pid_to_name(pid: u32) -> Option<String> {
    #[cfg(windows)]
    {
        let mut cache = PID_NAME_CACHE.lock().unwrap();
        if cache.is_none() {
            *cache = Some(build_pid_name_map());
        }
        cache
            .as_ref()
            .unwrap()
            .get(&pid)
            .cloned()
            .filter(|s| !s.is_empty())
    }
    #[cfg(not(windows))]
    {
        let _ = pid;
        None
    }
}

/// 连接级解析：代理端口 + 客户端 (IP, 端口) → 进程名。
/// 手机等远程设备（owning_pid = 0）解析不到，返回 None。
#[cfg(windows)]
pub fn resolve_app(proxy_port: u16, client_ip: &str, client_port: u16) -> Option<String> {
    let ip: Ipv4Addr = match parse_ipv4(client_ip) {
        Some(ip) => ip,
        None => return None, // IPv6（含 ::1 localhost）暂不查 v6 表
    };
    if client_port == 0 {
        return None;
    }
    let buf = get_tcp_table();
    let pid = parse_tcp_table(&buf, proxy_port, ip, client_port)?;
    if pid == 0 {
        return None; // 系统/远程设备连接
    }
    pid_to_name(pid)
}

/// 兼容解析 IPv4 / IPv4-mapped IPv6（`::ffff:127.0.0.1`）。
#[cfg(windows)]
fn parse_ipv4(s: &str) -> Option<Ipv4Addr> {
    if let Ok(ip) = s.parse::<Ipv4Addr>() {
        return Some(ip);
    }
    // ::ffff:a.b.c.d → a.b.c.d
    if let Some(tail) = s.rsplit(':').next() {
        if let Ok(ip) = tail.parse::<Ipv4Addr>() {
            return Some(ip);
        }
    }
    None
}

#[cfg(windows)]
fn get_tcp_table() -> Vec<u8> {
    use std::ptr;
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        GetExtendedTcpTable, TCP_TABLE_OWNER_PID_ALL,
    };
    unsafe {
        // 先取所需缓冲区大小
        let mut size: u32 = 0;
        GetExtendedTcpTable(ptr::null_mut(), &mut size, 0, 2, TCP_TABLE_OWNER_PID_ALL as i32, 0);
        let mut buf = vec![0u8; size as usize + 8];
        let rc = GetExtendedTcpTable(
            buf.as_mut_ptr() as *mut std::ffi::c_void,
            &mut size,
            0,
            2, // AF_INET
            TCP_TABLE_OWNER_PID_ALL as i32,
            0,
        );
        if rc == 0 {
            buf.truncate(size as usize);
            buf
        } else {
            Vec::new()
        }
    }
}

#[cfg(not(windows))]
pub fn resolve_app(_proxy_port: u16, _client_ip: &str, _client_port: u16) -> Option<String> {
    None
}

// ==================== 单元测试 ====================

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造一条 MIB_TCPROW_OWNER_PID 行（24 字节）。
    fn row(local_addr: [u8; 4], local_port: u16, remote_addr: [u8; 4], remote_port: u16, pid: u32) -> Vec<u8> {
        let mut r = Vec::with_capacity(24);
        r.extend_from_slice(&1u32.to_le_bytes()); // dwState ESTAB
        r.extend_from_slice(&local_addr);
        r.extend_from_slice(&local_port.to_be_bytes());
        r.extend_from_slice(&[0, 0]); // 填充
        r.extend_from_slice(&remote_addr);
        r.extend_from_slice(&remote_port.to_be_bytes());
        r.extend_from_slice(&[0, 0]); // 填充
        r.extend_from_slice(&pid.to_le_bytes());
        r
    }

    #[test]
    fn parse_tcp_table_finds_matching_row() {
        // dwNumEntries = 3（小端）
        let mut buf = vec![0u8; 4];
        buf[0] = 3;
        buf.extend(row([127, 0, 0, 1], 8080, [127, 0, 0, 1], 51234, 999));
        buf.extend(row([0, 0, 0, 0], 8888, [127, 0, 0, 1], 54321, 777));
        buf.extend(row([0, 0, 0, 0], 9999, [10, 0, 0, 5], 60000, 555));

        // 命中第二行：代理端口 8888 + 客户端 127.0.0.1:54321 → pid 777
        let pid = parse_tcp_table(&buf, 8888, Ipv4Addr::new(127, 0, 0, 1), 54321);
        assert_eq!(pid, Some(777));
    }

    #[test]
    fn parse_tcp_table_no_match() {
        let mut buf = vec![0u8; 4];
        buf[0] = 1;
        buf.extend(row([0, 0, 0, 0], 8888, [127, 0, 0, 1], 54321, 777));

        // 客户端端口不匹配
        assert_eq!(parse_tcp_table(&buf, 8888, Ipv4Addr::new(127, 0, 0, 1), 10000), None);
        // 代理端口不匹配
        assert_eq!(parse_tcp_table(&buf, 9999, Ipv4Addr::new(127, 0, 0, 1), 54321), None);
        // 空表
        assert_eq!(parse_tcp_table(&[0, 0, 0, 0], 8888, Ipv4Addr::new(127, 0, 0, 1), 54321), None);
        // 截断
        assert_eq!(parse_tcp_table(&[0, 0, 0, 1, 1], 8888, Ipv4Addr::new(127, 0, 0, 1), 54321), None);
    }

    /// 当前进程必然存在（ToolHelp 枚举里应有当前测试进程）。
    #[cfg(windows)]
    #[test]
    fn pid_to_name_resolves_current_process() {
        let pid = std::process::id();
        let name = pid_to_name(pid);
        assert!(name.is_some(), "当前进程 {pid} 应能解析出进程名");
        let n = name.unwrap();
        // lib 单测进程名形如 paxi-<hash>.exe
        assert!(n.to_lowercase().contains("paxi"), "进程名应是测试二进制：{n}");
    }
}