//! 规则引擎：匹配器 × 动作。
//!
//! 匹配器（全部条件同时满足才命中，空 = 任意）：
//! - host：glob 通配，如 `*.example.com`
//! - path：glob 通配，如 `/api/v1/*`
//! - method：精确（大小写不敏感）
//!
//! 动作：
//! - mock：直接返回构造的响应，不请求上游
//! - redirect：返回 3xx 重定向
//! - delay：请求转发前延迟（模拟慢客户端/超时）
//! - delay_response：收到上游响应后、返回客户端前延迟
//! - abort：拦截请求，返回 403
//!
//! 命中策略：按 priority 降序，第一条命中的规则生效（不链式）。
//! 命中计数在内存中累加，由后台任务周期性刷回 SQLite。

use crate::models::now_ms;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// 一条规则。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    /// 优先级：数值越大越先匹配
    #[serde(default)]
    pub priority: i32,
    #[serde(default)]
    pub matcher: RuleMatcher,
    pub action: RuleAction,
    #[serde(default)]
    pub hits: u64,
    #[serde(default)]
    pub updated_at: u128,
}

/// 匹配条件（字段为空/None 表示不限制）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RuleMatcher {
    /// host 通配符，如 `*.example.com`
    #[serde(default)]
    pub host: Option<String>,
    /// path 通配符，如 `/api/*`
    #[serde(default)]
    pub path: Option<String>,
    /// HTTP 方法，如 `GET`
    #[serde(default)]
    pub method: Option<String>,
}

/// 规则动作。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "params", rename_all = "snake_case")]
pub enum RuleAction {
    /// 返回 Mock 响应
    Mock {
        status: u16,
        content_type: String,
        body: String,
    },
    /// 重定向
    Redirect { to: String, status: u16 },
    /// 请求转发前延迟
    Delay { ms: u64 },
    /// 响应返回前延迟
    DelayResponse { ms: u64 },
    /// 拦截（403）
    Abort,
    /// 断点：挂起请求，等待前端放行（可修改）/拦截
    Breakpoint,
    /// 弱网模拟：首字节延迟 + 带宽限速 + 随机丢包（截断响应）
    Throttle {
        /// 带宽上限（KBytes/s，0 = 不限）
        kbps: u32,
        /// 首字节延迟（ms）
        delay_ms: u64,
        /// 丢包率（0-100，按响应随机截断）
        drop_pct: u8,
    },
}

impl Default for RuleAction {
    fn default() -> Self {
        RuleAction::Abort
    }
}

/// 命中结果。
pub struct MatchedRule {
    pub id: String,
    pub name: String,
    pub action: RuleAction,
}

/// 规则引擎：内存缓存（按优先级排序）+ SQLite 持久化 + 命中计数。
pub struct RulesEngine {
    /// 已按 priority 降序排序
    rules: Mutex<Vec<Rule>>,
    conn: Mutex<Connection>,
    /// 待刷回 DB 的命中计数：id -> hits
    dirty_hits: Mutex<HashMap<String, u64>>,
}

const RULE_SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS rule (
    id         TEXT PRIMARY KEY,
    name       TEXT NOT NULL,
    enabled    INTEGER NOT NULL DEFAULT 1,
    priority   INTEGER NOT NULL DEFAULT 0,
    matcher    TEXT NOT NULL DEFAULT '{}',
    action     TEXT NOT NULL,
    hits       INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL DEFAULT 0
);
"#;

impl RulesEngine {
    /// 打开（或创建）规则库，加载全部规则到内存并启动命中计数刷写任务。
    pub fn open(db_path: &Path) -> Result<Arc<Self>, String> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("创建数据目录失败：{e}"))?;
        }
        let conn = Connection::open(db_path).map_err(|e| format!("打开规则库失败：{e}"))?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(|e| e.to_string())?;
        conn.execute_batch(RULE_SCHEMA_SQL)
            .map_err(|e| format!("初始化规则表失败：{e}"))?;

        let engine = Arc::new(Self {
            rules: Mutex::new(load_rules(&conn)?),
            conn: Mutex::new(conn),
            dirty_hits: Mutex::new(HashMap::new()),
        });

        // 命中计数刷写任务：每 3 秒将脏计数写回 DB
        let flusher = engine.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(3)).await;
                flusher.flush_hits();
            }
        });

        Ok(engine)
    }

    /// 全部规则（已按优先级排序，返回克隆）。
    pub fn list(&self) -> Vec<Rule> {
        self.rules.lock().unwrap().clone()
    }

    /// 新增或更新规则（按 id upsert），并保持内存排序。
    pub fn upsert(&self, rule: Rule) -> Result<(), String> {
        {
            let conn = self.conn.lock().unwrap();
            let matcher_json =
                serde_json::to_string(&rule.matcher).map_err(|e| e.to_string())?;
            let action_json = serde_json::to_string(&rule.action).map_err(|e| e.to_string())?;
            conn.execute(
                "INSERT OR REPLACE INTO rule (id, name, enabled, priority, matcher, action, hits, updated_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
                params![
                    rule.id,
                    rule.name,
                    rule.enabled as i64,
                    rule.priority,
                    matcher_json,
                    action_json,
                    rule.hits as i64,
                    rule.updated_at as i64,
                ],
            )
            .map_err(|e| format!("保存规则失败：{e}"))?;
        }
        let mut rules = self.rules.lock().unwrap();
        match rules.iter_mut().find(|r| r.id == rule.id) {
            Some(existing) => *existing = rule,       // 保留内存命中数以传入值为准
            None => {
                rules.push(rule);
                rules.sort_by(|a, b| b.priority.cmp(&a.priority));
            }
        }
        Ok(())
    }

    /// 删除规则。
    pub fn delete(&self, id: &str) -> Result<(), String> {
        {
            let conn = self.conn.lock().unwrap();
            conn.execute("DELETE FROM rule WHERE id = ?1", params![id])
                .map_err(|e| format!("删除规则失败：{e}"))?;
        }
        self.rules.lock().unwrap().retain(|r| r.id != id);
        self.dirty_hits.lock().unwrap().remove(id);
        Ok(())
    }

    /// 匹配：按优先级找第一条命中的启用规则；命中则内存计数 + 标脏。
    pub fn match_rule(&self, host: &str, path: &str, method: &str) -> Option<MatchedRule> {
        let mut rules = self.rules.lock().unwrap();
        for r in rules.iter_mut() {
            if !r.enabled {
                continue;
            }
            if matcher_matches(&r.matcher, host, path, method) {
                r.hits += 1;
                let hits = r.hits;
                self.dirty_hits
                    .lock()
                    .unwrap()
                    .insert(r.id.clone(), hits);
                return Some(MatchedRule {
                    id: r.id.clone(),
                    name: r.name.clone(),
                    action: r.action.clone(),
                });
            }
        }
        None
    }

    /// 将脏命中计数刷回 DB。
    fn flush_hits(&self) {
        let dirty: Vec<(String, u64)> = {
            let mut d = self.dirty_hits.lock().unwrap();
            std::mem::take(&mut *d).into_iter().collect()
        };
        if dirty.is_empty() {
            return;
        }
        if let Ok(conn) = self.conn.lock() {
            for (id, hits) in dirty {
                let _ = conn.execute(
                    "UPDATE rule SET hits = ?2 WHERE id = ?1",
                    params![id, hits as i64],
                );
            }
        }
    }
}

/// 从 DB 加载全部规则并按优先级排序。
fn load_rules(conn: &Connection) -> Result<Vec<Rule>, String> {
    let mut stmt = conn
        .prepare("SELECT id, name, enabled, priority, matcher, action, hits, updated_at FROM rule")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)? != 0,
                row.get::<_, i64>(3)? as i32,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, i64>(6)? as u64,
                row.get::<_, i64>(7)? as u128,
            ))
        })
        .map_err(|e| e.to_string())?;

    let mut rules = Vec::new();
    for row in rows.flatten() {
        let (id, name, enabled, priority, matcher_json, action_json, hits, updated_at) = row;
        let matcher: RuleMatcher =
            serde_json::from_str(&matcher_json).unwrap_or_default();
        let action: RuleAction = match serde_json::from_str(&action_json) {
            Ok(a) => a,
            Err(_) => continue, // 兼容旧数据：动作解析失败则跳过
        };
        rules.push(Rule {
            id,
            name,
            enabled,
            priority,
            matcher,
            action,
            hits,
            updated_at,
        });
    }
    rules.sort_by(|a, b| b.priority.cmp(&a.priority));
    Ok(rules)
}

/// 判断匹配器是否命中（空字段不限制）。
fn matcher_matches(m: &RuleMatcher, host: &str, path: &str, method: &str) -> bool {
    if let Some(h) = m.host.as_deref() {
        let h = h.trim();
        if !h.is_empty() && !glob_match(h, host) {
            return false;
        }
    }
    if let Some(p) = m.path.as_deref() {
        let p = p.trim();
        if !p.is_empty() && !glob_match(p, path) {
            return false;
        }
    }
    if let Some(me) = m.method.as_deref() {
        let me = me.trim();
        if !me.is_empty() && !me.eq_ignore_ascii_case(method) {
            return false;
        }
    }
    true
}

/// glob 匹配：`*` 匹配任意字符序列（含空），`?` 匹配单个字符；大小写不敏感。
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let t: Vec<char> = text.to_lowercase().chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star_p, mut star_t) = (usize::MAX, 0usize);

    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star_p = pi;
            star_t = ti;
            pi += 1;
        } else if star_p != usize::MAX {
            pi = star_p + 1;
            star_t += 1;
            ti = star_t;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// 便捷：构造一条新规则（前端生成 id 也可，这里给默认值用）。
#[allow(dead_code)]
pub fn new_rule(name: &str) -> Rule {
    Rule {
        id: uuid::Uuid::new_v4().to_string(),
        name: name.to_string(),
        enabled: true,
        priority: 0,
        matcher: RuleMatcher::default(),
        action: RuleAction::Delay { ms: 1000 },
        hits: 0,
        updated_at: now_ms(),
    }
}
