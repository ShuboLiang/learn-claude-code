# Web Profile/模型选择器 实施计划

> **对 agentic worker 的要求：** 使用 superpowers:subagent-driven-development 或 superpowers:executing-plans 逐任务执行。步骤使用 checkbox (`- [ ]`) 语法跟踪。

**目标：** 在 Web 前端 Header 添加 profile 和模型下拉选择器，创建新会话时可选配 API 配置和模型。

**架构：** 后端新增 GET /config 端点暴露 profile 列表；Session 存储 profile_name/model；AgentApp 新增 `with_provider_and_model` 建造者方法；AppState 缓存 profile→provider 映射。前端 Header 添加两个下拉框，选中值在创建会话时传给后端。

**技术栈：** Rust (axum/tokio/serde)、React + TypeScript + Zustand + Tailwind CSS

---

## 文件变更总览

| 文件 | 操作 | 职责 |
|------|------|------|
| `crates/core/src/infra/config.rs` | 已完成 | models: Vec<String> + resolve_model() |
| `crates/core/src/api/mod.rs` | 已完成 | 使用 resolve_model() |
| `crates/core/src/agent.rs` | 修改 | 新增 with_provider_and_model 方法 |
| `crates/server/src/session.rs` | 修改 | Session/SessionRecord 加 profile_name, model |
| `crates/server/src/routes.rs` | 修改 | GET /config、POST /sessions 扩展、send_message 改造 |
| `crates/server/src/main.rs` | 修改 | AppState 新增 config + providers 缓存 |
| `web/src/api/client.ts` | 修改 | 新增 getConfig API + createSession 扩展 |
| `web/src/types/wire.ts` | 修改 | 新增 ProfileInfo 类型 |
| `web/src/store/chat.ts` | 修改 | 新增 profiles/selectedProfile/selectedModel |
| `web/src/App.tsx` | 修改 | Header 新增选择器 UI |

---

### Task 1: AgentApp::with_provider_and_model

**文件:**
- 修改: `crates/core/src/agent.rs`

- [ ] **Step 1: 在 impl AgentApp 块中新增方法（在 with_extension 之后）**

```rust
/// 替换 provider 和 model（用于会话级 profile 切换）
pub fn with_provider_and_model(mut self, client: crate::api::LlmProvider, model: String) -> Self {
    self.client = client;
    self.model = model;
    self
}
```

- [ ] **Step 2: 编译验证**

```bash
cargo check -p rust-agent-core 2>&1
```

预期: 编译通过，无新错误

- [ ] **Step 3: 提交**

```bash
git add crates/core/src/agent.rs
git commit -m "feat: AgentApp 新增 with_provider_and_model 建造者方法"
```

---

### Task 2: Session 存储 profile_name 和 model

**文件:**
- 修改: `crates/server/src/session.rs`

- [ ] **Step 1: Session struct 新增字段**

```rust
pub struct Session {
    pub id: String,
    pub context: ContextService,
    pub created_at: DateTime<Utc>,
    pub last_active: DateTime<Utc>,
    pub working_dir: PathBuf,
    pub profile_name: String,   // 使用的 profile 名称
    pub model: String,          // 使用的模型 ID
}
```

- [ ] **Step 2: SessionRecord 新增字段**

```rust
struct SessionRecord {
    version: u32,
    id: String,
    created_at: DateTime<Utc>,
    last_active: DateTime<Utc>,
    messages: Vec<rust_agent_core::api::types::ApiMessage>,
    #[serde(default = "default_working_dir")]
    working_dir: PathBuf,
    #[serde(default = "default_empty_string")]
    profile_name: String,
    #[serde(default = "default_empty_string")]
    model: String,
}

fn default_empty_string() -> String {
    String::new()
}
```

- [ ] **Step 3: From<&Session> for SessionRecord 同步新字段**

```rust
impl From<&Session> for SessionRecord {
    fn from(session: &Session) -> Self {
        Self {
            // ...existing...
            profile_name: session.profile_name.clone(),
            model: session.model.clone(),
        }
    }
}
```

- [ ] **Step 4: SessionRecord::into_session 同步新字段**

```rust
fn into_session(self) -> Session {
    // ...existing...
    Session {
        // ...existing...
        profile_name: self.profile_name,
        model: self.model,
    }
}
```

- [ ] **Step 5: SessionStore::create 接受 profile_name 和 model 参数**

```rust
pub async fn create(
    &self,
    working_dir: PathBuf,
    profile_name: String,
    model: String,
) -> Arc<RwLock<Session>> {
    // 构造 Session 时带上新字段
}
```

- [ ] **Step 6: 编译验证**

```bash
cargo check -p rust-agent-server 2>&1
```

预期: 编译失败（create 调用处签名不匹配），下一步修复

- [ ] **Step 7: 提交**

```bash
git add crates/server/src/session.rs
git commit -m "feat: Session 存储 profile_name 和 model 字段"
```

---

### Task 3: 后端路由改造

**文件:**
- 修改: `crates/server/src/routes.rs`

- [ ] **Step 1: 新增 GET /config 端点（在 routes 函数中注册，在 health_check 之后）**

```rust
.route("/config", get(get_config))
```

```rust
/// 配置信息（脱敏）
#[derive(Serialize)]
struct ProfileInfo {
    name: String,
    provider: String,
    models: Vec<String>,
}

/// GET /config — 返回可用 profiles 和默认值
async fn get_config(State(state): State<AppState>) -> impl IntoResponse {
    let profiles: Vec<ProfileInfo> = state
        .config
        .profiles
        .iter()
        .map(|p| ProfileInfo {
            name: p.name.clone(),
            provider: p.provider.clone(),
            models: p.models.clone(),
        })
        .collect();
    let current = state.config.current_profile().ok();
    Json(serde_json::json!({
        "default_profile": state.config.default_profile,
        "current_profile": current.map(|p| p.name.as_str()).unwrap_or(""),
        "current_model": state.agent.model(),
        "profiles": profiles,
    }))
    .into_response()
}
```

- [ ] **Step 2: 修改 CreateSessionRequest 接受 profile 和 model**

```rust
#[derive(Deserialize)]
struct CreateSessionRequest {
    #[serde(default)]
    working_dir: Option<String>,
    #[serde(default)]
    profile: Option<String>,
    #[serde(default)]
    model: Option<String>,
}
```

- [ ] **Step 3: 改 create_session 解析 profile/model**

```rust
async fn create_session(State(state): State<AppState>, Json(body): Json<CreateSessionRequest>) -> impl IntoResponse {
    let working_dir = /* ...existing... */;

    // 解析 profile 和 model
    let profile_name = body.profile
        .or_else(|| state.config.current_profile().ok().map(|p| p.name.clone()))
        .unwrap_or_default();
    let model = body.model
        .or_else(|| {
            state.config.current_profile().ok()
                .and_then(|p| p.resolve_model().ok())
        })
        .unwrap_or_default();

    let session_arc = state.store.create(working_dir, profile_name, model).await;
    // ...
}
```

- [ ] **Step 4: 改 send_message 按会话配置创建 agent 变体**

```rust
async fn send_message(...) -> impl IntoResponse {
    // ...existing session lookup...

    tokio::spawn(async move {
        let session = session_arc.read().await;
        let cwd = session.working_dir.clone();
        let session_profile = session.profile_name.clone();
        let session_model = session.model.clone();
        drop(session);

        // 按会话配置获取或创建 provider
        let agent = if session_profile.is_empty()
            || session_profile == state.config.current_profile().ok().map(|p| p.name.as_str()).unwrap_or("")
        {
            agent.clone() // 使用全局默认 agent
        } else {
            // 为不同 profile 创建临时 agent 变体
            let provider = state.get_or_create_provider(&session_profile);
            match provider {
                Ok(p) => agent.clone().with_provider_and_model(p, session_model),
                Err(e) => {
                    let _ = event_tx.send(AgentEvent::Error {
                        code: "profile_error".to_owned(),
                        message: format!("{e:#}"),
                    }).await;
                    let _ = event_tx.send(AgentEvent::Done).await;
                    return;
                }
            }
        };

        // 后续使用 agent 代替 state.agent...
        let mut session = session_arc.write().await;
        if let Err(e) = agent
            .handle_user_turn(&mut session.context, &content, Some(&cwd), event_tx.clone())
            .await
        { /* ... */ }
    });
}
```

- [ ] **Step 5: 编译验证**

```bash
cargo check -p rust-agent-server 2>&1
```

预期: 依赖 AppState 新字段（下一步添加），暂时编译失败

- [ ] **Step 6: 提交**

```bash
git add crates/server/src/routes.rs
git commit -m "feat: 新增 GET /config 端点 + 会话级 profile/model 支持"
```

---

### Task 4: AppState 扩展 + main.rs 改造

**文件:**
- 修改: `crates/server/src/main.rs`

- [ ] **Step 1: AppState 新增字段**

```rust
use rust_agent_core::infra::config::AppConfig;
use rust_agent_core::api::LlmProvider;
use dashmap::DashMap;

pub struct AppState {
    pub store: SessionStore,
    pub agent: Arc<AgentApp>,
    pub bot_registry: Arc<BotRegistry>,
    pub config: Arc<AppConfig>,                              // 缓存的全局配置
    pub providers: Arc<DashMap<String, LlmProvider>>,        // profile_name -> provider 缓存
}
```

- [ ] **Step 2: AppState 新增 get_or_create_provider 方法**

```rust
impl AppState {
    pub fn get_or_create_provider(&self, profile_name: &str) -> AgentResult<LlmProvider> {
        // 先查缓存
        if let Some(p) = self.providers.get(profile_name) {
            return Ok(p.clone());
        }
        // 查找 profile 配置
        let profile = self.config.find_profile(profile_name)
            .context(format!("profile '{}' 不存在", profile_name))?;
        // 创建 provider
        let provider = match profile.provider.to_lowercase().as_str() {
            "openai" => LlmProvider::OpenAI(
                openai::OpenAIClient::new(&profile.api_key, &profile.base_url)?
            ),
            _ => LlmProvider::Anthropic(
                anthropic::AnthropicClient::new(&profile.api_key, &profile.base_url)?
            ),
        };
        self.providers.insert(profile_name.to_owned(), provider.clone());
        Ok(provider)
    }
}
```

- [ ] **Step 3: main.rs 初始化新字段**

```rust
use rust_agent_core::infra::config::AppConfig;

let config = Arc::new(AppConfig::load()?);
let providers: Arc<DashMap<String, LlmProvider>> = Arc::new(DashMap::new());

let app_state = routes::AppState {
    store,
    agent,
    bot_registry,
    config,
    providers,
};
```

- [ ] **Step 4: 编译验证**

```bash
cargo check -p rust-agent-server 2>&1
```

预期: 编译通过，无错误

- [ ] **Step 5: 提交**

```bash
git add crates/server/src/main.rs crates/server/src/routes.rs
git commit -m "feat: AppState 缓存配置和 provider 实例，支持会话级 profile 切换"
```

---

### Task 5: 前端类型 + API 层

**文件:**
- 修改: `web/src/types/wire.ts`
- 修改: `web/src/api/client.ts`

- [ ] **Step 1: wire.ts 新增 ProfileInfo 类型**

```ts
export interface ProfileInfo {
  name: string
  provider: string
  models: string[]
}
```

- [ ] **Step 2: client.ts 新增 getConfig + 扩展 createSession**

```ts
export interface ConfigResponse {
  default_profile: string
  current_profile: string
  current_model: string
  profiles: ProfileInfo[]
}

export function getConfig(): Promise<ConfigResponse> {
  return request<ConfigResponse>('/config')
}
```

修改 createSession：

```ts
export function createSession(
  workingDir?: string,
  profile?: string,
  model?: string,
): Promise<{ id: string; working_dir: string }> {
  return request('/sessions', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      working_dir: workingDir || null,
      profile: profile || null,
      model: model || null,
    }),
  })
}
```

- [ ] **Step 3: 提交**

```bash
git add web/src/types/wire.ts web/src/api/client.ts
git commit -m "feat: 前端新增 getConfig API + ProfileInfo 类型 + createSession 扩展"
```

---

### Task 6: ChatStore 新增 profile/model 状态

**文件:**
- 修改: `web/src/store/chat.ts`

- [ ] **Step 1: ChatState 新增字段**

```ts
import type { ProfileInfo } from '@/types/wire'

interface ChatState {
  // ...existing...
  profiles: ProfileInfo[]
  selectedProfile: string
  selectedModel: string
}
```

- [ ] **Step 2: 初始值**

```ts
profiles: [],
selectedProfile: '',
selectedModel: '',
```

- [ ] **Step 3: 新增 loadConfig action**

```ts
async loadConfig() {
  try {
    const config = await api.getConfig()
    set((s) => {
      s.profiles = config.profiles
      s.selectedProfile = config.current_profile || config.default_profile
      // 找到当前 profile 的 models，默认第一个
      const p = config.profiles.find(p => p.name === s.selectedProfile)
      s.selectedModel = config.current_model || p?.models[0] || ''
    })
  } catch (err) {
    console.error('Failed to load config:', err)
  }
},
```

- [ ] **Step 4: 修改 createSession 传参**

```ts
async createSession(workingDir?: string) {
  const { selectedProfile, selectedModel } = get()
  const { id, working_dir } = await api.createSession(workingDir, selectedProfile, selectedModel)
  // ... rest unchanged
},
```

- [ ] **Step 5: 新增 setSelectedProfile / setSelectedModel actions**

```ts
setSelectedProfile(profile: string) {
  set((s) => {
    s.selectedProfile = profile
    // 切换 profile 时自动选第一个模型
    const p = s.profiles.find(p => p.name === profile)
    s.selectedModel = p?.models[0] || ''
  })
},

setSelectedModel(model: string) {
  set((s) => {
    s.selectedModel = model
  })
},
```

- [ ] **Step 6: 提交**

```bash
git add web/src/store/chat.ts
git commit -m "feat: ChatStore 新增 profile/model 选择状态和 loadConfig"
```

---

### Task 7: Header 选择器 UI

**文件:**
- 修改: `web/src/App.tsx`

- [ ] **Step 1: 新增 import**

```tsx
import { ChevronDown } from 'lucide-react'
```

- [ ] **Step 2: 从 store 读取状态**

```tsx
const profiles = useChatStore((s) => s.profiles)
const selectedProfile = useChatStore((s) => s.selectedProfile)
const selectedModel = useChatStore((s) => s.selectedModel)
const setSelectedProfile = useChatStore((s) => s.setSelectedProfile)
const setSelectedModel = useChatStore((s) => s.setSelectedModel)
const loadConfig = useChatStore((s) => s.loadConfig)
```

- [ ] **Step 3: useEffect 加载配置**

```tsx
useEffect(() => {
  loadConfig()
}, [loadConfig])
```

- [ ] **Step 4: 当前 profile 对应的 models 列表**

```tsx
const currentModels = profiles.find((p) => p.name === selectedProfile)?.models || []
```

- [ ] **Step 5: Header 中插入选择器（在标题和快捷键之间）**

```tsx
<header className="flex h-11 shrink-0 items-center gap-3 border-b bg-background/80 backdrop-blur px-4">
  <div className="flex items-center gap-2">
    <span className="...">R</span>
    <h1 className="...">rust<span className="text-primary">-agent</span></h1>
  </div>

  {/* 新增：profile + model 选择器 */}
  <div className="flex items-center gap-2 ml-4">
    <div className="relative">
      <select
        value={selectedProfile}
        onChange={(e) => setSelectedProfile(e.target.value)}
        className="h-7 rounded-md border bg-background px-2 pr-6 text-xs appearance-none cursor-pointer hover:border-primary/50 focus:outline-none focus:ring-1 focus:ring-primary"
      >
        {profiles.map((p) => (
          <option key={p.name} value={p.name}>{p.name}</option>
        ))}
      </select>
      <ChevronDown className="absolute right-1.5 top-1/2 -translate-y-1/2 h-3 w-3 text-muted-foreground pointer-events-none" />
    </div>

    <div className="relative">
      <select
        value={selectedModel}
        onChange={(e) => setSelectedModel(e.target.value)}
        className="h-7 rounded-md border bg-background px-2 pr-6 text-xs appearance-none cursor-pointer hover:border-primary/50 focus:outline-none focus:ring-1 focus:ring-primary"
      >
        {currentModels.map((m) => (
          <option key={m} value={m}>{m}</option>
        ))}
      </select>
      <ChevronDown className="absolute right-1.5 top-1/2 -translate-y-1/2 h-3 w-3 text-muted-foreground pointer-events-none" />
    </div>
  </div>

  <span className="ml-auto text-[10px] text-muted-foreground/60">
    Ctrl+N new · Ctrl+L clear
  </span>
</header>
```

- [ ] **Step 7: 提交**

```bash
git add web/src/App.tsx
git commit -m "feat: Header 新增 profile 和模型下拉选择器"
```

---

### Task 8: 端到端验证

- [ ] **Step 1: 编译后端**

```bash
cargo check 2>&1
```

预期: 全部编译通过

- [ ] **Step 2: 检查前端类型**

```bash
cd web && npx tsc --noEmit 2>&1
```

预期: 无类型错误

- [ ] **Step 3: 手动测试**

启动服务后：
1. 访问 `GET /config` 确认返回正确
2. 创建会话时不传 profile/model，确认使用默认值
3. 创建会话时传 profile/model，确认会话使用指定配置
4. 同时创建两个会话用不同模型，确认互不干扰

- [ ] **Step 4: 提交**

```bash
git commit -m "chore: 端到端验证通过"
```
