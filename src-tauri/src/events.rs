//! 事件批量推送：将高频流量事件合并（100ms 窗口）后推送给前端。
//!
//! 前端监听：
//! - `traffic://new`       Vec<RequestMeta>  新增记录
//! - `traffic://update`    Vec<RequestMeta>  记录更新（如 WS 结束后的耗时）
//! - `traffic://ws-frames` Vec<(String, WsFrame)>  WebSocket 帧批次

use crate::models::{RequestMeta, WsFrame};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

/// 批量刷新窗口。
const FLUSH_INTERVAL_MS: u64 = 100;

struct Pending {
    metas: Vec<RequestMeta>,
    updates: Vec<RequestMeta>,
    frames: Vec<(String, WsFrame)>,
}

struct Inner {
    /// None = 无头模式（测试）：只入队不 emit
    app: Option<AppHandle>,
    pending: Mutex<Pending>,
    flushing: AtomicBool,
}

/// 事件中心（克隆廉价，内部共享）。
#[derive(Clone)]
pub struct EventHub(Arc<Inner>);

use std::sync::Arc;

impl EventHub {
    /// 创建事件中心。app 用于向前端窗口 emit。
    pub fn new(app: AppHandle) -> Self {
        Self::with_app(Some(app))
    }

    /// 无头模式（集成测试用）：事件只入队合并，不向前端 emit。
    pub fn headless() -> Self {
        Self::with_app(None)
    }

    fn with_app(app: Option<AppHandle>) -> Self {
        Self(Arc::new(Inner {
            app,
            pending: Mutex::new(Pending {
                metas: Vec::new(),
                updates: Vec::new(),
                frames: Vec::new(),
            }),
            flushing: AtomicBool::new(false),
        }))
    }

    /// 新增一条记录事件。
    pub fn push_meta(&self, meta: RequestMeta) {
        let mut pending = self.0.pending.lock().unwrap();
        pending.metas.push(meta);
        drop(pending);
        Self::schedule(&self.0);
    }

    /// 记录更新事件（如 WebSocket 连接结束）。
    pub fn push_update(&self, meta: RequestMeta) {
        let mut pending = self.0.pending.lock().unwrap();
        pending.updates.push(meta);
        drop(pending);
        Self::schedule(&self.0);
    }

    /// WebSocket 帧事件。
    pub fn push_frame(&self, record_id: &str, frame: WsFrame) {
        let mut pending = self.0.pending.lock().unwrap();
        pending.frames.push((record_id.to_string(), frame));
        drop(pending);
        Self::schedule(&self.0);
    }

    /// 断点命中事件（低频，立即推送不进合并窗口）。
    pub fn push_breakpoint(&self, info: crate::proxy::BreakpointInfo) {
        if let Some(app) = &self.0.app {
            let _ = app.emit("breakpoint://hit", &info);
        }
    }

    /// 若无在途 flush 任务，安排一个 100ms 后的批量推送。
    fn schedule(inner: &Arc<Inner>) {
        if inner.flushing.swap(true, Ordering::AcqRel) {
            return; // 已有任务在途
        }
        let inner = inner.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_millis(FLUSH_INTERVAL_MS)).await;
            // 先复位再取数：保证复位后新入队的数据一定有下一个任务接管
            inner.flushing.store(false, Ordering::SeqCst);
            let pending = {
                let mut p = inner.pending.lock().unwrap();
                std::mem::replace(
                    &mut *p,
                    Pending {
                        metas: Vec::new(),
                        updates: Vec::new(),
                        frames: Vec::new(),
                    },
                )
            };
            if !pending.metas.is_empty() {
                if let Some(app) = &inner.app {
                    let _ = app.emit("traffic://new", &pending.metas);
                }
            }
            if !pending.updates.is_empty() {
                if let Some(app) = &inner.app {
                    let _ = app.emit("traffic://update", &pending.updates);
                }
            }
            if !pending.frames.is_empty() {
                if let Some(app) = &inner.app {
                    let _ = app.emit("traffic://ws-frames", &pending.frames);
                }
            }
        });
    }
}
