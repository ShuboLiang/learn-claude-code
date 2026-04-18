# Thinking 内容块兼容设计

- 日期：2026-04-19
- 主题：兼容 Anthropic 响应中的 `thinking` 内容块
- 状态：已确认，待用户复核

## 背景

当前 Anthropic 响应反序列化使用 `ResponseContentBlock` 枚举，仅支持以下内容块：

- `text`
- `tool_use`

当上游返回如下内容块时：

```json
{
  "type": "thinking",
  "thinking": "..."
}
```

`serde_json::from_str` 会因为未知枚举分支而失败，导致整次消息解析报错。该问题已经通过错误日志定位，属于协议兼容性缺失，而不是请求失败。

## 目标

本次修改只解决一个问题：

1. 允许 Anthropic 响应中的 `thinking` 内容块被成功反序列化

本次修改明确不做以下事情：

1. 不向最终用户展示 `thinking`
2. 不把 `thinking` 参与 `final_text()` 拼接
3. 不让 `thinking` 参与工具调用提取逻辑
4. 不额外设计通用未知内容块兜底机制

## 方案对比

### 方案 1：新增 `Thinking` 枚举变体并忽略它（采用）

在 `ResponseContentBlock` 中新增：

- `Thinking { thinking: String }`

然后保持现有消费逻辑：

- `final_text()` 只提取 `Text`
- 工具调用提取逻辑只匹配 `ToolUse`
- `Thinking` 仅作为可反序列化、可存储的内容块存在

优点：

- 改动最小
- 与现有数据结构最一致
- 风险最低
- 不引入额外协议猜测

缺点：

- 未来如果出现更多新内容块，仍需要单独兼容

### 方案 2：增加未知块兜底变体（不采用）

例如增加统一的未知块兜底结构，用于吞掉所有未知类型。

优点：

- 对未来未知协议有更强容错性

缺点：

- 容易掩盖协议变化
- 当前需求只针对 `thinking`，属于过度设计
- 调试时不如显式建模清晰

### 方案 3：在反序列化前过滤 `thinking`（不采用）

先将响应读成 `serde_json::Value`，手动删除 `thinking` 块，再转换为现有结构。

优点：

- 不需要修改枚举结构

缺点：

- 逻辑更绕
- 可维护性差
- 与现有类型驱动设计不一致

## 采用设计

### 数据结构

在 `crates/core/src/api/types.rs` 的 `ResponseContentBlock` 中新增：

- `Thinking { thinking: String }`

该变体仅用于表达来自 Anthropic 的思考块。

### 运行时行为

1. **反序列化阶段**
   - `thinking` 块可以被正常解析，不再导致整条消息失败

2. **文本提取阶段**
   - `ProviderResponse::final_text()` 继续只拼接 `Text`
   - `Thinking` 不参与最终文本输出

3. **工具调用阶段**
   - 工具提取逻辑继续只处理 `ToolUse`
   - `Thinking` 不参与工具执行调度

4. **上下文存储阶段**
   - `assistant_blocks()` 会保留 `Thinking` 块，因为它仍属于合法响应内容的一部分
   - 但现有消费路径不会把它当成用户可见文本或工具调用

## 影响范围

### 需要修改

- `crates/core/src/api/types.rs`
  - 为 `ResponseContentBlock` 新增 `Thinking` 变体
  - 更新 `final_text()` 的匹配分支

- `crates/core/src/api/anthropic.rs`
  - 补充针对 `thinking` 的失败测试与通过测试

### 不需要修改

- `crates/core/src/agent.rs`
  - 当前工具调用提取逻辑已只处理 `ToolUse`，无需改动

- `crates/core/src/api/openai.rs`
  - 本次不处理 OpenAI 侧协议扩展

## 测试设计

采用 TDD。

### 失败测试

新增一个最小测试，构造包含以下内容的响应 JSON：

- 一个 `thinking` 块
- 一个 `tool_use` 块

当前行为应为：

- 解析失败

目标行为应为：

- 解析成功
- `final_text()` 不包含 thinking 内容
- 工具调用块仍能保留在解析结果中

### 验证点

至少验证以下事实：

1. 含 `thinking` 的响应可成功解析
2. `thinking` 不会出现在 `final_text()` 中
3. `tool_use` 仍可被保留下来

## 错误处理

本次修改不改变已有错误链设计，只修复 `thinking` 导致的解析失败。

如果后续再出现新的内容块类型，系统仍会通过已有错误日志暴露实际响应体，便于继续扩展兼容。

## 范围边界

本次工作只做 Anthropic 响应协议兼容，不做以下扩展：

- 不展示 reasoning/thinking 给用户
- 不重构消息上下文格式
- 不添加通用未知块适配层
- 不改变 OpenAI 响应转换逻辑

## 实施后预期结果

当上游返回 `thinking + tool_use` 组合内容块时：

- 请求不再因反序列化失败而中断
- 工具调用流程可继续执行
- 最终展示文本行为保持不变
- 系统对 `thinking` 具备基础兼容能力
