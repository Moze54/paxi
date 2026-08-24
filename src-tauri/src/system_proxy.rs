//! 系统代理管理：启动/停止代理时设置与恢复 Windows 系统代理。
//!
//! 原理：Windows 系统代理配置在注册表
//! `HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings`：
//! - ProxyEnable（0/1）
//! - ProxyServer（"ip:port"）
//!
//! 设置后需调用 WinINet 的 InternetSetOption 通知系统刷新，或通过
//! `InternetSetOption` API。这里用注册表 + 广播系统消息的方式。

use std::sync::Mutex;

/// 保存设置代理前的原始配置，以便停止时恢复。
#[cfg(target_os = "windows")]
#[derive(Clone, Default)]
struct OriginalProxy {
    enable: u32,
    server: String,
    override_list: String,
}

/// 全局保存原始代理配置。
#[cfg(target_os = "windows")]
static ORIGINAL_PROXY: Mutex<Option<OriginalProxy>> = Mutex::new(None);

/// 注册表路径。
const INTERNET_SETTINGS: &str = r"Software\Microsoft\Windows\CurrentVersion\Internet Settings";

/// 设置系统代理为 127.0.0.1:port，并保存原始配置。
#[cfg(target_os = "windows")]
pub fn set_system_proxy(port: u16) -> Result<(), String> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu
        .create_subkey(INTERNET_SETTINGS)
        .map_err(|e| e.to_string())?;

    // 保存原始配置
    let orig_enable: u32 = key.get_value("ProxyEnable").unwrap_or(0);
    let orig_server: String = key.get_value("ProxyServer").unwrap_or_default();
    let orig_override: String = key.get_value("ProxyOverride").unwrap_or_default();
    *ORIGINAL_PROXY.lock().unwrap() = Some(OriginalProxy {
        enable: orig_enable,
        server: orig_server,
        override_list: orig_override,
    });

    // 设置新代理
    key.set_value("ProxyEnable", &1u32).map_err(|e| e.to_string())?;
    key.set_value("ProxyServer", &format!("127.0.0.1:{}", port))
        .map_err(|e| e.to_string())?;
    // 本地地址不走代理
    key.set_value("ProxyOverride", &"localhost;127.*;<local>")
        .map_err(|e| e.to_string())?;

    // 通知系统刷新代理设置
    notify_proxy_change();

    Ok(())
}

/// 恢复原始系统代理配置。
#[cfg(target_os = "windows")]
pub fn restore_system_proxy() -> Result<(), String> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let orig = ORIGINAL_PROXY.lock().unwrap().clone();
    let Some(orig) = orig else {
        return Ok(()); // 没有保存过，无需恢复
    };

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu
        .create_subkey(INTERNET_SETTINGS)
        .map_err(|e| e.to_string())?;

    key.set_value("ProxyEnable", &orig.enable).map_err(|e| e.to_string())?;
    if orig.server.is_empty() {
        let _ = key.delete_value("ProxyServer");
    } else {
        key.set_value("ProxyServer", &orig.server).map_err(|e| e.to_string())?;
    }
    if orig.override_list.is_empty() {
        let _ = key.delete_value("ProxyOverride");
    } else {
        key.set_value("ProxyOverride", &orig.override_list)
            .map_err(|e| e.to_string())?;
    }

    *ORIGINAL_PROXY.lock().unwrap() = None;
    notify_proxy_change();

    Ok(())
}

/// 通知系统刷新代理设置。
#[cfg(target_os = "windows")]
fn notify_proxy_change() {
    // 通过 WinINet 的 InternetSetOption 广播代理设置变更。
    // 使用动态加载 wininet.dll 避免依赖编译期 FFI。
    use std::os::windows::ffi::OsStrExt;

    const INTERNET_OPTION_SETTINGS_CHANGED: u32 = 39;
    const INTERNET_OPTION_REFRESH: u32 = 37;

    // 用 std::ffi 动态声明
    type SetOptionFn = unsafe extern "system" fn(
        *mut std::ffi::c_void,
        u32,
        *mut std::ffi::c_void,
        u32,
    ) -> i32;

    unsafe {
        let wininet_name: Vec<u16> = std::ffi::OsStr::new("wininet.dll")
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let kernel_name: Vec<u16> = std::ffi::OsStr::new("kernel32.dll")
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        extern "system" {
            fn LoadLibraryW(name: *const u16) -> *mut std::ffi::c_void;
            fn GetProcAddress(h: *mut std::ffi::c_void, name: *const u8) -> *mut std::ffi::c_void;
        }

        let kernel = LoadLibraryW(kernel_name.as_ptr());
        let _ = kernel;

        let wininet = LoadLibraryW(wininet_name.as_ptr());
        if wininet.is_null() {
            return;
        }

        let name = b"InternetSetOptionW\0";
        let proc = GetProcAddress(wininet, name.as_ptr());
        if proc.is_null() {
            return;
        }

        let set_option: SetOptionFn = std::mem::transmute(proc);
        let _ = set_option(
            std::ptr::null_mut(),
            INTERNET_OPTION_SETTINGS_CHANGED,
            std::ptr::null_mut(),
            0,
        );
        let _ = set_option(
            std::ptr::null_mut(),
            INTERNET_OPTION_REFRESH,
            std::ptr::null_mut(),
            0,
        );
    }
}

// ===== macOS：通过 networksetup 设置系统代理 =====

#[cfg(target_os = "macos")]
pub fn set_system_proxy(port: u16) -> Result<(), String> {
    let services = list_network_services()?;
    let mut saved: Vec<(String, bool, String, u16)> = Vec::new(); // (service, web_enabled, host, port)

    for service in &services {
        // 保存 HTTP 代理原状态
        if let Some((enabled, host, p)) = get_proxy_state(&format!("getwebproxy"), service) {
            saved.push((service.clone(), enabled, host, p));
        }
        // 设置 HTTP / HTTPS 代理指向本机
        run_networksetup(&["-setwebproxy", service, "127.0.0.1", &port.to_string()])?;
        run_networksetup(&["-setsecurewebproxy", service, "127.0.0.1", &port.to_string()])?;
        // 本地地址不走代理
        run_networksetup(&[
            "-setproxybypassdomains",
            service,
            "localhost",
            "127.0.0.1",
            "*.local",
            "169.254/16",
        ])?;
    }

    *ORIGINAL_PROXY.lock().unwrap() = Some(saved);
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn restore_system_proxy() -> Result<(), String> {
    let saved = ORIGINAL_PROXY.lock().unwrap().take();
    let Some(saved) = saved else {
        return Ok(());
    };

    for (service, was_enabled, host, port) in saved {
        if was_enabled {
            let host = if host.is_empty() { "127.0.0.1".to_string() } else { host };
            let _ = run_networksetup(&["-setwebproxy", &service, &host, &port.to_string()]);
        } else {
            let _ = run_networksetup(&["-setwebproxystate", &service, "off"]);
        }
    }
    Ok(())
}

/// 列出所有网络服务（跳过首行提示与禁用项）。
#[cfg(target_os = "macos")]
fn list_network_services() -> Result<Vec<String>, String> {
    let out = run_networksetup(&["-listallnetworkservices"])?;
    Ok(out
        .lines()
        .skip(1)
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('*'))
        .map(|l| l.to_string())
        .collect())
}

/// 读取某服务某类代理的状态：返回 (是否启用, 服务器, 端口)。
#[cfg(target_os = "macos")]
fn get_proxy_state(kind: &str, service: &str) -> Option<(bool, String, u16)> {
    let out = run_networksetup(&[&format!("-{kind}"), service]).ok()?;
    let mut enabled = false;
    let mut host = String::new();
    let mut port = 0u16;
    for line in out.lines() {
        if let Some(v) = line.strip_prefix("Enabled:") {
            enabled = v.trim().eq_ignore_ascii_case("yes");
        } else if let Some(v) = line.strip_prefix("Server:") {
            host = v.trim().to_string();
        } else if let Some(v) = line.strip_prefix("Port:") {
            port = v.trim().parse().unwrap_or(0);
        }
    }
    Some((enabled, host, port))
}

/// 执行 networksetup，成功返回 stdout。
#[cfg(target_os = "macos")]
fn run_networksetup(args: &[&str]) -> Result<String, String> {
    use std::process::Command;
    let out = Command::new("networksetup")
        .args(args)
        .output()
        .map_err(|e| format!("执行 networksetup 失败：{e}"))?;
    if !out.status.success() {
        return Err(format!(
            "networksetup {:?} 失败：{}",
            args,
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// macOS 保存的原始代理状态。
#[cfg(target_os = "macos")]
static ORIGINAL_PROXY: Mutex<Option<Vec<(String, bool, String, u16)>>> = Mutex::new(None);

// ===== 其他平台暂不支持 =====

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn set_system_proxy(_port: u16) -> Result<(), String> {
    Err("当前平台尚未支持自动设置系统代理".to_string())
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn restore_system_proxy() -> Result<(), String> {
    Ok(())
}
