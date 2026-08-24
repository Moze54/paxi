//! 流量存储抽象：trait 定义 + SQLite 实现。

pub mod sqlite;

use crate::models::{RequestMeta, RequestRecord, WsFrame};

/// 流量存储接口。
///
/// 实现需保证线程安全；写入可以异步落盘，但写入后应尽快可读。
pub trait TrafficStore: Send + Sync {
    /// 插入一条完整记录（请求+响应已完成），返回列表 meta。
    fn insert(&self, record: RequestRecord) -> RequestMeta;

    /// WebSocket 连接结束：更新耗时与错误信息。
    fn finish_websocket(&self, id: &str, duration_ms: u128, error: Option<String>);

    /// 追加一帧 WebSocket 记录。
    fn insert_frame(&self, record_id: &str, frame: WsFrame);

    /// 获取列表（最新在前，有上限）。
    fn list(&self) -> Vec<RequestMeta>;

    /// 获取单条详情。
    fn get(&self, id: &str) -> Option<RequestRecord>;

    /// 获取某条记录的全部 WebSocket 帧。
    fn frames(&self, id: &str) -> Vec<WsFrame>;

    /// 清空所有记录（含 body 文件与帧）。
    fn clear(&self);
}
