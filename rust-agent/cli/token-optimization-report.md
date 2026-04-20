# LLM Agent Token 优化方案汇总报告

## 概述

Token 优化本质上是一个 **context engineering（上下文工程）问题**，而非单纯的 prompt 缩短问题。实际的成本大头往往来自臃肿的上下文、重复发送的对话历史、以及不必要的工具调用，而不是 system prompt 本身。

---

## 一、Prompt 压缩 (Prompt Compression)

### 1.1 静态压缩
- **精简 system prompt**：移除冗余描述、示例和重复指令
- **结构化指令**：用 JSON/YAML 格式代替自然语言描述，减少 token 数
- **工具描述优化**：工具 (tool/function) 的描述是 token 消耗大户，只保留必要参数说明

### 1.2 动态压缩 — LLMLingua / LLMLingua-2 (Microsoft)
```python
from llmlingua import PromptCompressor
compressor = PromptCompressor(
    model_name="microsoft/llmlingua-2-xlm-roberta-large-meetingbank",
    use_llmlingua2=True
)
compressed = compressor.compress(long_prompt, target_token=2000)
```
- **压缩比**：可达 50%-80%
- **适用场景**：长文档、长对话历史、RAG 检索结果

### 1.3 选择性压缩
- **关键信息保留**：代码、数字、关键指令保持原样
- **冗余内容压缩**：背景描述、解释性文字可以大幅压缩
- **分层压缩**：对话历史分层处理（近期对话保留完整，远期对话压缩）

---

## 二、上下文管理 (Context Window Management)

### 2.1 对话历史裁剪
- **滑动窗口**：只保留最近 N 轮对话
- **摘要替换**：将早期对话替换为摘要（summarization），用一次小的 LLM 调用替换大量 token
- **关键信息提取**：从历史中提取关键事实/决策，丢弃冗余对话

### 2.2 上下文预算 (Context Budgets)
- 为不同交互类型设置最大 token 限制
- 自动裁剪超限内容
- 优先级排序：用户最新输入 > 工具结果 > 历史对话 > system prompt

### 2.3 选择性上下文注入
- 不要每次都发送完整的知识库/代码库上下文
- 根据当前任务按需注入 (按需检索)
- 使用 RAG 替代全量上下文注入

---

## 三、缓存策略 (Caching)

### 3.1 Prompt Caching (LLM 提供商原生)
- **Anthropic**：缓存对话前缀，多轮对话可减少 ~75% 输入 token 成本
- **OpenAI**：支持 prompt caching（自动缓存相同前缀）
- **Google Gemini**：支持上下文缓存 (context caching)
- **原理**：相同或相似的 prompt 前缀会被缓存，后续请求只需支付缓存命中费用

### 3.2 语义缓存 (Semantic Caching)
```python
# 使用 GPTCache 等工具
# 基于 embedding 相似度匹配缓存结果
# 相似度阈值 > 0.95 时直接返回缓存结果
```
- 对相似问题直接返回缓存结果，跳过 LLM 调用
- **节省**：100% token 消耗（命中时）

### 3.3 工具调用缓存
- 缓存常见工具调用的结果
- 相同参数调用直接返回缓存结果
- 适用于：文件读取、API 查询、数据库查询等确定性操作

### 3.4 多层缓存架构
```
L1: 精确匹配缓存 (exact match) → 100% 命中，零 token
L2: 语义缓存 (semantic similarity) → ~95% 相似度返回缓存
L3: Prompt Caching (LLM 提供商) → 前缀复用，成本降低 50-90%
```

---

## 四、模型路由 (Model Routing)

### 4.1 分层模型策略
| 任务类型 | 推荐模型 | 成本 |
|---------|---------|------|
| 简单分类/路由 | 小模型 (GPT-4o-mini, Claude Haiku) | $0.15-0.80/M |
| 中等复杂度 | 中模型 (GPT-4o, Claude Sonnet) | $3.00-15.00/M |
| 复杂推理 | 大模型 (o1, Claude Opus, DeepSeek-R1) | $15.00-60.00/M |

### 4.2 智能路由实现
```
用户请求 → 分类器 → 
  ├─ 简单任务 → 小模型 (快且便宜)
  ├─ 中等任务 → 中模型
  └─ 复杂任务 → 大模型
```

### 4.3 思考模式优化
- 对于不需要深度推理的任务，**关闭 thinking/reasoning 模式**
- 设置 `thinkingBudget = 0` 可以显著减少输出 token

---

## 五、工具与函数调用优化

### 5.1 减少不必要的工具调用
- Agent 经常因为过度思考而调用不需要的工具
- 在 system prompt 中明确何时调用工具，何时直接回答
- 使用 `parallel_tool_calls` 减少多轮交互

### 5.2 工具结果裁剪
- 工具返回结果可能很大（如整个文件内容、长 API 响应）
- 只提取与当前任务相关的部分
- 对大文件先做摘要再注入上下文

### 5.3 工具描述精简
```
# 优化前 (~50 tokens)
"This tool allows you to read the contents of a file at a specified path. 
 You should provide the full path to the file you want to read..."

# 优化后 (~15 tokens)
"Read file contents at path."
```

---

## 六、RAG 优化

### 6.1 精确检索
- 提高检索精度，减少注入的不相关上下文
- 使用 hybrid search (BM25 + embedding)
- 重排序 (re-ranking) 筛选最相关文档

### 6.2 检索结果压缩
- 对检索到的文档块做压缩后再注入
- 提取式压缩比生成式压缩更快且保真度更高

### 6.3 避免全量文档注入
- 不要将整个文档塞入 prompt
- 使用段落级检索而非文档级检索

---

## 七、架构层面优化

### 7.1 子代理 (Sub-agent) 隔离
- 不同任务使用独立的 agent 实例，避免上下文污染
- 子代理完成后只返回精简结果，不传递完整上下文

### 7.2 流式处理 (Streaming)
- 使用 stream 模式，可以在获取到足够信息时提前终止
- 减少不必要的全量生成

### 7.3 提前终止
- 设置最大迭代次数
- 当任务完成时主动终止 agent 循环
- 监控 token 使用量，超限时降级处理

### 7.4 批处理
- 将多个相似请求合并为一次调用
- 适用于：批量分类、批量摘要等场景

---

## 八、数据层面优化

### 8.1 数值精度优化
- LLM 很少需要毫秒级精度
- 格式化数字可减少 30%-40% 的数值 token 消耗
```
# 优化前: 3.141592653589793 (多个 token)
# 优化后: 3.14 (更少 token)
```

### 8.2 代码上下文优化
- 只发送相关函数/方法，而非整个文件
- 使用 AST 分析定位相关代码
- 去除注释和空白（在不影响理解的前提下）

### 8.3 格式优化
- 去除 JSON 中的空格和换行（紧凑格式）
- 使用缩写代替长字段名（在工具定义中）

---

## 九、监控与分析

### 9.1 Token 遥测 (Telemetry)
- 记录每次 LLM 调用的输入/输出 token 数
- 分析 token 分布：system prompt / history / tools / output 各占多少
- 识别 token 消耗大户

### 9.2 成本追踪
```
每次调用记录:
- model used
- input tokens
- output tokens  
- cost
- task type
- latency
```

### 9.3 持续优化
- 基于遥测数据持续调整策略
- A/B 测试不同压缩方案的效果

---

## 十、推荐实施优先级

| 优先级 | 策略 | 预期节省 | 实施难度 |
|-------|------|---------|---------|
| P0 | 对话历史裁剪 + 摘要 | 30-50% | 低 |
| P0 | Prompt Caching（利用提供商） | 50-75% | 低 |
| P1 | 工具描述精简 | 10-20% | 低 |
| P1 | 模型路由 | 40-70% | 中 |
| P1 | 工具结果裁剪 | 20-40% | 中 |
| P2 | 语义缓存 | 15-30% | 中 |
| P2 | 动态压缩 (LLMLingua) | 50-80% | 高 |
| P3 | 子代理隔离架构 | 20-50% | 高 |

---

## 参考来源

1. [Fast.io - AI Agent Token Cost Optimization 2026](https://fast.io/resources/ai-agent-token-cost-optimization/)
2. [SitePoint - Context Compression Techniques](https://www.sitepoint.com/optimizing-token-usage-context-compression-techniques/)
3. [Microsoft LLMLingua-2](https://github.com/microsoft/LLMLingua)
4. [LogRocket - 10 ways to cut token usage](https://blog.logrocket.com/stop-wasting-ai-tokens-10-ways-to-reduce-usage/)
5. [TowardsAI - How I Optimize Tokens While Building AI Agents](https://pub.towardsai.net/how-i-optimize-tokens-while-building-ai-agents-without-killing-output-quality-804fedfb54fd)
6. [Redis - LLM Token Optimization](https://redis.io/blog/llm-token-optimization-speed-up-apps/)
7. [Token Optimize - Complete Guide 2026](https://www.tokenoptimize.dev/guides/llm-token-optimization-strategies)
8. [arXiv - Local-Splitter: Seven Tactics for Reducing Token Usage](https://arxiv.org/html/2604.12301v1)
9. [arXiv - An Evaluation of Prompt Caching for Long-Horizon Agentic Tasks](https://arxiv.org/html/2601.06007v2)
10. [Maxim AI - Context Engineering for AI Agents](https://www.getmaxim.ai/articles/context-engineering-for-ai-agents-production-optimization-strategies/)
