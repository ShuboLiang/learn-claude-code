# OpenAI 联网可用性测试 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为 `crates/core/src/api/openai.rs` 增加一个可通过 `cargo test` 运行的真实联网测试，在环境变量齐全时真实请求 OpenAI 兼容接口，在环境变量缺失时跳过并给出中文说明。

**Architecture:** 在 `OpenAIClient` 所在文件内增加一个 `#[cfg(test)]` 测试模块，复用现有 `OpenAIClient::new`、`ApiMessage`、`ProviderRequest` 和 `ProviderResponse::final_text()`。测试采用显式环境变量门控：缺少必需变量时直接返回，避免影响常规测试；变量齐全时发送一组最小 `messages` 请求并断言返回非空文本，聚焦验证联通性而非完整行为覆盖。

**Tech Stack:** Rust、Tokio、reqwest、serde_json、cargo test

---

## 文件结构

- Modify: `crates/core/src/api/openai.rs`
  - 责任：保留 OpenAI 客户端实现，并新增紧邻实现的联网测试模块。
- Reuse: `crates/core/src/api/types.rs`
  - 责任：提供 `ApiMessage`、`ProviderRequest`、`ProviderResponse::final_text()` 供测试构造请求和提取返回文本。
- Reference: `docs/superpowers/specs/2026-04-18-openai-live-test-design.md`
  - 责任：作为本计划的规格来源，确保实现范围不外溢。

## 实施任务

### Task 1: 添加环境变量门控的失败测试

**Files:**
- Modify: `crates/core/src/api/openai.rs`
- Reference: `crates/core/src/api/types.rs:19-132`
- Spec: `docs/superpowers/specs/2026-04-18-openai-live-test-design.md`

- [ ] **Step 1: 在 `openai.rs` 末尾写入失败测试模块**

```rust
#[cfg(test)]
mod tests {
    use super::OpenAIClient;
    use crate::api::types::{ApiMessage, ProviderRequest};

    fn live_test_env(name: &str) -> Option<String> {
        std::env::var(name).ok().filter(|value| !value.trim().is_empty())
    }

    #[tokio::test]
    async fn openai_live_test_returns_non_empty_text_when_env_is_configured() {
        let api_key = match live_test_env("OPENAI_API_KEY") {
            Some(value) => value,
            None => {
                eprintln!("[跳过] 未设置 OPENAI_API_KEY，跳过 OpenAI 联网测试");
                return;
            }
        };
        let base_url = match live_test_env("OPENAI_BASE_URL") {
            Some(value) => value,
            None => {
                eprintln!("[跳过] 未设置 OPENAI_BASE_URL，跳过 OpenAI 联网测试");
                return;
            }
        };
        let model = match live_test_env("OPENAI_LIVE_TEST_MODEL") {
            Some(value) => value,
            None => {
                eprintln!("[跳过] 未设置 OPENAI_LIVE_TEST_MODEL，跳过 OpenAI 联网测试");
                return;
            }
        };

        let system = live_test_env("OPENAI_LIVE_TEST_SYSTEM")
            .unwrap_or_else(|| "你是一个接口联通性测试助手。".to_owned());
        let prompt = live_test_env("OPENAI_LIVE_TEST_PROMPT")
            .unwrap_or_else(|| "请只回复 pong".to_owned());

        let client = OpenAIClient::new(&api_key, &base_url)
            .expect("客户端应该可以基于环境变量成功初始化");
        let messages = vec![ApiMessage::user_text(prompt)];
        let request = ProviderRequest {
            model: &model,
            system: &system,
            messages: &messages,
            tools: &[],
            max_tokens: 32,
        };

        let response = client.create_message(&request).await
            .expect("真实联网请求应该成功返回");
        let final_text = response.final_text();

        assert!(
            !final_text.trim().is_empty(),
            "真实联网请求成功后，返回文本不应为空"
        );
    }
}
```

- [ ] **Step 2: 运行单个测试，确认它先失败**

Run: `cargo test -p rust-agent-core openai_live_test_returns_non_empty_text_when_env_is_configured -- --nocapture`
Expected: FAIL，报错原因应是测试模块尚未编译通过、导入路径不正确，或现有实现尚未满足测试入口需要；如果本机未配置环境变量而直接 PASS，需要先临时设置 3 个必需环境变量再重跑，以证明测试真正覆盖了联网路径。

- [ ] **Step 3: 修正测试代码到最小可编译版本，不改生产逻辑**

```rust
#[cfg(test)]
mod tests {
    use super::OpenAIClient;
    use crate::api::{ApiMessage, ProviderRequest};

    fn live_test_env(name: &str) -> Option<String> {
        std::env::var(name).ok().filter(|value| !value.trim().is_empty())
    }

    #[tokio::test]
    async fn openai_live_test_returns_non_empty_text_when_env_is_configured() {
        let api_key = match live_test_env("OPENAI_API_KEY") {
            Some(value) => value,
            None => {
                eprintln!("[跳过] 未设置 OPENAI_API_KEY，跳过 OpenAI 联网测试");
                return;
            }
        };
        let base_url = match live_test_env("OPENAI_BASE_URL") {
            Some(value) => value,
            None => {
                eprintln!("[跳过] 未设置 OPENAI_BASE_URL，跳过 OpenAI 联网测试");
                return;
            }
        };
        let model = match live_test_env("OPENAI_LIVE_TEST_MODEL") {
            Some(value) => value,
            None => {
                eprintln!("[跳过] 未设置 OPENAI_LIVE_TEST_MODEL，跳过 OpenAI 联网测试");
                return;
            }
        };

        let system = live_test_env("OPENAI_LIVE_TEST_SYSTEM")
            .unwrap_or_else(|| "你是一个接口联通性测试助手。".to_owned());
        let prompt = live_test_env("OPENAI_LIVE_TEST_PROMPT")
            .unwrap_or_else(|| "请只回复 pong".to_owned());

        let client = OpenAIClient::new(&api_key, &base_url)
            .expect("客户端应该可以基于环境变量成功初始化");
        let messages = vec![ApiMessage::user_text(prompt)];
        let request = ProviderRequest {
            model: &model,
            system: &system,
            messages: &messages,
            tools: &[],
            max_tokens: 32,
        };

        let response = client.create_message(&request).await
            .expect("真实联网请求应该成功返回");
        let final_text = response.final_text();

        assert!(
            !final_text.trim().is_empty(),
            "真实联网请求成功后，返回文本不应为空"
        );
    }
}
```

- [ ] **Step 4: 再次运行单个测试，确认转绿或按门控跳过**

Run: `cargo test -p rust-agent-core openai_live_test_returns_non_empty_text_when_env_is_configured -- --nocapture`
Expected:
- 配置齐全时：PASS，且不会出现 panic
- 缺少环境变量时：PASS，但输出 `[跳过] ...` 中文说明

- [ ] **Step 5: 提交这一小步变更**

```bash
git add crates/core/src/api/openai.rs
git commit -m "test: add live OpenAI connectivity check"
```

### Task 2: 验证测试对真实返回文本的最小可用性约束

**Files:**
- Modify: `crates/core/src/api/openai.rs`
- Reference: `crates/core/src/api/openai.rs:325-543`

- [ ] **Step 1: 写一个更具体的失败断言，要求返回文本去空白后非空**

```rust
assert!(
    !response.final_text().trim().is_empty(),
    "真实联网请求成功后，返回文本不应为空"
);
```

- [ ] **Step 2: 运行单个测试，确认断言能捕获空文本场景**

Run: `cargo test -p rust-agent-core openai_live_test_returns_non_empty_text_when_env_is_configured -- --nocapture`
Expected: 如果接口只返回空白文本，FAIL with `真实联网请求成功后，返回文本不应为空`；如果接口返回正常文本，PASS。

- [ ] **Step 3: 采用最小实现，只保留 `final_text().trim().is_empty()` 的断言**

```rust
let response = client.create_message(&request).await
    .expect("真实联网请求应该成功返回");

assert!(
    !response.final_text().trim().is_empty(),
    "真实联网请求成功后，返回文本不应为空"
);
```

- [ ] **Step 4: 运行单个测试，确认稳定通过**

Run: `cargo test -p rust-agent-core openai_live_test_returns_non_empty_text_when_env_is_configured -- --nocapture`
Expected: PASS；如果本机环境未配置，则输出中文跳过提示后 PASS。

- [ ] **Step 5: 提交这一小步变更**

```bash
git add crates/core/src/api/openai.rs
git commit -m "test: assert non-empty text in OpenAI live check"
```

### Task 3: 运行针对性验证，确认不破坏现有测试

**Files:**
- Verify: `crates/core/src/api/openai.rs`
- Verify: `crates/core/src/tools/mod.rs:221-260`

- [ ] **Step 1: 运行新增测试并保留输出**

Run: `cargo test -p rust-agent-core openai_live_test_returns_non_empty_text_when_env_is_configured -- --nocapture`
Expected: PASS，且在缺少环境变量时有中文跳过提示。

- [ ] **Step 2: 运行 `openai.rs` 相关测试范围，确认没有连带失败**

Run: `cargo test -p rust-agent-core openai -- --nocapture`
Expected: PASS，新增测试之外不存在新的失败。

- [ ] **Step 3: 运行 `rust-agent-core` 全量测试，确认无回归**

Run: `cargo test -p rust-agent-core -- --nocapture`
Expected: PASS；已有 `#[ignore]` 测试继续保持忽略状态；输出中不应出现新的 panic 或编译错误。

- [ ] **Step 4: 运行 Clippy 做最小静态检查**

Run: `cargo clippy -p rust-agent-core --all-targets -- -D warnings`
Expected: PASS，无新增 warning。

- [ ] **Step 5: 提交验证后的最终变更**

```bash
git add crates/core/src/api/openai.rs
git commit -m "test: add live verification for OpenAI client"
```

## 规格覆盖检查

- 规格要求“`cargo test` 可跑的真实联网测试” → Task 1
- 规格要求“缺少环境变量时跳过，不误报失败” → Task 1
- 规格要求“发送最小 `messages` 请求” → Task 1
- 规格要求“只验证最小可用性，不做复杂快照” → Task 2
- 规格要求“用户能直接判断接口能不能用” → Task 3

## 自查结论

- 无 `TBD`、`TODO`、`implement later` 等占位词
- 后续任务中的测试名、环境变量名、断言文本与前文保持一致
- 所有代码步骤都给出了具体代码，所有执行步骤都给出了具体命令和预期结果
