# Curl 工具设计文档

## 背景与目标

Agent 已可通过 `bash` 工具执行 `curl` 命令，但存在以下问题：
1. **JSON 体验差** — Agent 需要手写 curl 命令字符串，参数拼接易出错
2. **安全性不足** — bash curl 无访问限制，可请求任意地址
3. **错误处理粗糙** — 网络错误以 raw stderr 返回，Agent 难以解析

本设计为 `core` crate 封装一个专用的 `curl` 工具，让 Agent 以结构化 JSON 参数发起 HTTP 请求，获得格式化响应。

## 需求摘要

- 通用 HTTP 请求工具，支持 GET/POST/PUT/DELETE/PATCH
- 默认精简模式（仅返回 body），可选完整模式（含 status/headers/body）
- 安全策略：默认空黑名单，用户通过 `config.json` 配置禁止访问的域名/网段
- 复用现有 `reqwest` 依赖，不引入新库
- 与现有 `AgentToolbox` 工具体系无缝集成

## 架构设计

### 模块结构

```
crates/core/src/tools/
  ├── mod.rs          — 新增 "curl" 分支到 dispatch
  ├── curl.rs         — CurlClient + 安全策略 + 请求/响应类型
  └── schemas.rs      — 新增 curl 的 JSON Schema
```

### 核心组件

#### `CurlClient`

```rust
pub struct CurlClient {
    http: reqwest::Client,
    blacklist: Vec<BlacklistEntry>,
}

enum BlacklistEntry {
    Exact(String),      // "localhost"
    Wildcard(String),   // "192.168.*"
    Regex(Regex),       // regex:10\.\d+\.\d+\.\d+
}
```

- `CurlClient` 从 `AppConfig` 加载配置，包括用户自定义黑名单
- 默认黑名单为空列表，不限制任何地址
- 内部复用 `reqwest::Client` 实例，支持连接复用

#### 对外接口

```rust
impl CurlClient {
    /// 从 AppConfig 加载（默认黑名单为空）
    pub fn from_config(config: &AppConfig) -> Self;
    
    /// 精简模式：只返回 body 文本
    pub async fn fetch(&self, url: &str, method: &str, body: Option<Value>) -> AgentResult<String>;
    
    /// 完整模式：返回结构化响应
    pub async fn request(&self, req: CurlRequest) -> AgentResult<CurlResponse>;
}
```

### 工具集成

`AgentToolbox::dispatch()` 中新增 `"curl"` 分支：

```rust
"curl" => {
    let url = required_string(input, "url")?;
    let method = input.get("method").and_then(|v| v.as_str()).unwrap_or("GET");
    let detailed = input.get("detailed").and_then(|v| v.as_bool()).unwrap_or(false);
    let headers = input.get("headers").cloned();
    let body = input.get("body").and_then(|v| v.as_str());
    let json_body = input.get("json").cloned();
    let timeout = input.get("timeout").and_then(|v| v.as_u64()).unwrap_or(30) as u64;
    
    self.curl_client.execute(url, method, headers, body, json_body, timeout, detailed).await?
}
```

`AgentToolbox` 新增 `curl_client` 字段，在 `new()` 时初始化。

## 数据格式

### 请求 Schema

```json
{
  "name": "curl",
  "description": "发起 HTTP 请求。默认返回响应 body，detailed=true 时返回完整信息。",
  "input_schema": {
    "type": "object",
    "required": ["url"],
    "properties": {
      "url": { "type": "string", "description": "请求地址" },
      "method": { "type": "string", "enum": ["GET", "POST", "PUT", "DELETE", "PATCH"], "default": "GET" },
      "headers": { "type": "object", "description": "可选的请求头" },
      "body": { "type": "string", "description": "原始 body 文本" },
      "json": { "type": "object", "description": "JSON body（自动设置 Content-Type: application/json）" },
      "timeout": { "type": "integer", "description": "超时秒数（默认 30）" },
      "detailed": { "type": "boolean", "description": "返回完整响应信息", "default": false }
    }
  }
}
```

### 精简模式响应（detailed=false）

直接返回响应 body 文本字符串。若 HTTP 状态码 ≥ 400，则在 body 前附加 `[HTTP {status}] ` 前缀。

### 完整模式响应（detailed=true）

```json
{
  "status": 200,
  "status_text": "OK",
  "headers": { "content-type": "application/json" },
  "body": "响应体文本",
  "elapsed_ms": 156
}
```

## 安全策略

### 黑名单配置

- **默认**：空列表，不限制任何地址
- **配置位置**：`~/.rust-agent/config.json` 中 `curl_blacklist` 字段
- **匹配规则**：
  - `localhost` — 精确匹配 host
  - `192.168.*` — 通配符匹配（`*` 匹配任意字符）
  - `regex:10\.\d+\.\d+\.\d+` — 正则匹配（以 `regex:` 前缀标识）

### 配置示例

```json
{
  "curl_blacklist": [
    "localhost",
    "127.0.0.1",
    "192.168.*",
    "10.*",
    "regex:^172\\.(1[6-9]|2\\d|3[01])\\."
  ]
}
```

### 校验时机

在请求发起前解析 URL，提取 host，依次匹配黑名单规则。命中则立即返回错误，不发起网络请求。

## 错误处理

| 场景 | 行为 |
|---|---|
| URL 解析失败 | `AgentResult::Err("无效的 URL: {url}")` |
| 命中黑名单 | `AgentResult::Err("URL 被安全策略禁止: {host}")` |
| 网络超时 | `AgentResult::Err("请求超时（{timeout} 秒）")` |
| DNS 失败 | `AgentResult::Err("无法解析主机: {host}")` |
| HTTP ≥ 400（精简模式） | `Ok("[HTTP {status}] {body}")` |
| HTTP ≥ 400（完整模式） | 正常返回 `CurlResponse`，由调用方判断 status |
| 响应体 > 10MB | 截断并附加 `[响应体超过 10MB，已截断]` 提示 |

## 边界处理

- **响应体大小限制**：默认 10MB，超限截断
- **Content-Type 自动设置**：若使用 `json` 参数，自动添加 `Content-Type: application/json`
- **编码处理**：优先 UTF-8 解码，失败时尝试 `encoding_rs` 自动检测
- **超时**：默认 30 秒，通过 `timeout` 参数覆盖

## 测试策略

### 单元测试（`tools/curl.rs` 内）

- 黑名单匹配逻辑：精确、通配符、正则三种模式
- URL 解析与 host 提取
- 精简/完整模式响应格式化
- 错误消息格式

### 集成测试

- 对 httpbin.org 发起真实请求（标记 `#[ignore]`，手动触发）
- 或启动本地 mock server 测试完整请求链路

## 依赖与整合点

- **无新增依赖**：复用已有的 `reqwest`、`serde_json`、`regex`、`encoding_rs`
- **修改文件**：
  - `tools/mod.rs`：dispatch 新增 `"curl"` 分支
  - `tools/schemas.rs`：新增 curl 的 JSON Schema
  - `infra/config.rs`：新增 `curl_blacklist: Option<Vec<String>>` 字段
  - `Cargo.toml`：无需修改

## 非目标（明确排除）

- 不支持 HTTP/2 Server Push、WebSocket
- 不支持 Cookie Jar 自动管理
- 不支持代理自动发现（如需代理，通过 `HTTP_PROXY` 环境变量由 reqwest 处理）
- 不支持文件上传（multipart/form-data）
- 白名单模式（仅支持黑名单）
