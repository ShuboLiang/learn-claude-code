# Web 前端 Profile/模型选择器

## 目标

在 Web 前端 Header 中添加 profile 和模型的下拉选择器，用户创建新会话时可选择使用的 API 配置和模型。

## 约束

- 仅在创建新会话时切换 profile/模型，会话中途不可切换
- 多个会话可以并发使用不同的 profile/模型，互不干扰
- 旧配置格式 `"model": "str"` 向后兼容，自动转为 `["str"]`

## 后端改动

### 1. 新增 GET /config

返回所有 profiles（隐藏 api_key）和当前默认值：

```json
{
  "default_profile": "my-api",
  "profiles": [
    { "name": "my-api", "provider": "openai", "models": ["gpt-4o", "gpt-4o-mini"] }
  ]
}
```

### 2. Session 新增 profile_name / model 字段

```rust
pub struct Session {
    // ...existing...
    pub profile_name: String,
    pub model: String,
}
```

SessionRecord 同步新增，序列化到磁盘。

### 3. POST /sessions 接受可选参数

```json
{ "working_dir": ".", "profile": "my-api", "model": "gpt-4o-mini" }
```

不传则 fallback 到服务器全局默认。

### 4. AgentApp::with_provider_and_model

遵循现有 `with_skills`/`with_extension` 建造者模式：

```rust
pub fn with_provider_and_model(self, client: LlmProvider, model: String) -> Self
```

### 5. AppState 缓存 providers

```rust
pub struct AppState {
    // ...existing...
    pub config: Arc<AppConfig>,                          // 缓存的配置
    pub providers: Arc<DashMap<String, LlmProvider>>,    // profile_name -> provider
}
```

### 6. send_message 按会话配置创建 agent 变体

```rust
let session = session_arc.read().await;
let provider = state.get_provider(&session.profile_name)?;  // 缓存查找
let agent = state.agent.clone().with_provider_and_model(provider, session.model.clone());
```

## 前端改动

### 1. Header 组件新增选择器

```
[R] rust-agent  [profile ▾]  [model ▾]           Ctrl+N...
```

- profile 下拉框：列出所有 profile 名称
- model 下拉框：跟随 profile 变化，显示对应 models 列表
- 页面加载时 GET /config 获取选项

### 2. ChatStore 新增状态

```ts
interface ChatState {
  profiles: ProfileInfo[]
  selectedProfile: string
  selectedModel: string
}
```

### 3. 创建会话时传参

createSession 将 selectedProfile 和 selectedModel 传给 POST /sessions。

## 不改动

- AgentApp 核心对话逻辑
- SSE 事件格式
- 会话中途切换（后续迭代）
- Bot 相关路由
