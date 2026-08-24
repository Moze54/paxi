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
#[derive(Clone, Default)]
struct OriginalProxy {
    enable: u32,
    server: String,
    override_list: String,
}

/// 全局保存原始代理配置。
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

// 非 Windows 平台的空实现。
#[cfg(not(target_os = "windows"))]
pub fn set_system_proxy(_port: u16) -> Result<(), String> {
    Err("当前平台尚未支持自动设置系统代理".to_string())
}

#[cfg(not(target_os = "windows"))]
pub fn restore_system_proxy() -> Result<(), String> {
    Ok(())
}
