//! 统计分析：从流量库聚合统计信息（前端统计面板数据源）。

use crate::models::RequestMeta;
use crate::storage::TrafficStore;
use serde::Serialize;
use std::collections::HashMap;

/// 统计结果（前端图表数据）。
#[derive(Default, Serialize)]
pub struct Stats {
    /// 记录总数
    pub total: u64,
    /// 成功（2xx/3xx）与失败（0/4xx/5xx）计数
    pub succeeded: u64,
    pub failed: u64,
    /// 状态码分布：status -> count
    pub status_dist: Vec<(String, u64)>,
    /// 方法分布
    pub method_dist: Vec<(String, u64)>,
    /// 协议分布
    pub scheme_dist: Vec<(String, u64)>,
    /// 域名 TOP N：host -> count
    pub host_top: Vec<(String, u64)>,
    /// 平均/最大耗时（ms）
    pub avg_duration_ms: u64,
    pub max_duration_ms: u64,
    /// 24 小时时间线：共 24 桶（按 started_at 小时对齐）
    pub timeline: Vec<u64>,
}

/// 聚合全部记录。
pub fn compute(store: &dyn TrafficStore) -> Stats {
    let metas = store.list();
    let mut stats = Stats::default();
    stats.total = metas.len() as u64;

    let mut status_map: HashMap<String, u64> = HashMap::new();
    let mut method_map: HashMap<String, u64> = HashMap::new();
    let mut scheme_map: HashMap<String, u64> = HashMap::new();
    let mut host_map: HashMap<String, u64> = HashMap::new();
    let mut timeline: Vec<u64> = vec![0; 24];
    let mut total_duration: u128 = 0;
    let mut max_duration: u128 = 0;

    let now = crate::models::now_ms() as i64;
    for m in metas.iter() {
        // 状态码分布 + 成功/失败
        let status_key = match m.status {
            0 => "error".to_string(),
            s => s.to_string(),
        };
        *status_map.entry(status_key).or_insert(0) += 1;
        if m.status == 0 || m.status >= 400 {
            stats.failed += 1;
        } else {
            stats.succeeded += 1;
        }

        *method_map.entry(m.method.to_uppercase()).or_insert(0) += 1;
        let scheme = if m.is_websocket {
            "ws".to_string()
        } else {
            m.scheme.clone()
        };
        *scheme_map.entry(scheme).or_insert(0) += 1;

        *host_map.entry(m.host.clone()).or_insert(0) += 1;

        // 耗时
        total_duration += m.duration_ms as u128;
        if m.duration_ms > max_duration {
            max_duration = m.duration_ms;
        }

        // 24h 时间线：按 started_at 相对 now 的小时
        let hours_ago = (now - m.started_at as i64) / 3_600_000;
        if hours_ago >= 0 && (hours_ago as usize) < 24 {
            timeline[hours_ago as usize] += 1;
        }
    }

    stats.total = metas.len() as u64;
    stats.status_dist = top_entries(status_map);
    // 状态码升序排列更直观
    stats.status_dist.sort_by(|a, b| {
        let ka = if a.0 == "error" { 999 } else { a.0.parse().unwrap_or(0) };
        let kb = if b.0 == "error" { 999 } else { b.0.parse().unwrap_or(0) };
        ka.cmp(&kb)
    });
    stats.method_dist = top_entries(method_map);
    stats.scheme_dist = top_entries(scheme_map);
    stats.host_top = top_entries(host_map);
    stats.avg_duration_ms = if metas.is_empty() {
        0
    } else {
        (total_duration / metas.len() as u128) as u64
    };
    stats.max_duration_ms = max_duration as u64;
    stats.timeline = timeline;

    stats
}

/// HashMap → 排序条目（count 降序）。
fn top_entries(map: HashMap<String, u64>) -> Vec<(String, u64)> {
    let mut v: Vec<(String, u64)> = map.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    v
}

/// 便捷工具：请求 meta 列表是否为空。
pub fn is_empty(metas: &[RequestMeta]) -> bool {
    metas.is_empty()
}