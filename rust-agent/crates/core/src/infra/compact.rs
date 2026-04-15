//! 向后兼容模块：所有压缩逻辑已迁移到 `context::compact`
//!
//! 此模块保留 re-export 以避免破坏外部依赖，后续版本将移除。

pub use crate::context::compact::*;
