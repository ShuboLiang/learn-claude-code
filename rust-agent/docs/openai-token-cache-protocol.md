# OpenAI API Token Count and Cache Protocol

## Overview

OpenAI's API returns detailed token usage information in the response object, including breakdowns for cached tokens, reasoning tokens, and other specialized token types.

## Response Format

### Chat Completions API

The standard Chat Completions API returns token information in the `usage` object:

```json
{
  "usage": {
    "prompt_tokens": 1117,
    "completion_tokens": 46,
    "total_tokens": 1163,
    "prompt_tokens_details": {
      "cached_tokens": 0,
      "audio_tokens": 0
    },
    "completion_tokens_details": {
      "reasoning_tokens": 0,
      "audio_tokens": 0,
      "accepted_prediction_tokens": 0,
      "rejected_prediction_tokens": 0
    }
  }
}
```

### Responses API

The newer Responses API uses slightly different field names:

```json
{
  "usage": {
    "input_tokens": 1289,
    "input_tokens_details": {
      "cached_tokens": 0
    },
    "output_tokens": 685,
    "output_tokens_details": {
      "reasoning_tokens": 640
    },
    "total_tokens": 1974
  }
}
```

## Token Count Fields

### Basic Token Counts

- **`prompt_tokens`** (Chat Completions) / **`input_tokens`** (Responses API): Number of tokens in the input prompt
- **`completion_tokens`** (Chat Completions) / **`output_tokens`** (Responses API): Number of tokens in the generated completion
- **`total_tokens`**: Total number of tokens used (prompt + completion)

### Detailed Token Breakdowns

#### Prompt/Input Token Details (`prompt_tokens_details` / `input_tokens_details`)

- **`cached_tokens`**: Number of tokens that were served from cache (discounted pricing)
- **`audio_tokens`**: Number of audio input tokens (for audio processing)

#### Completion/Output Token Details (`completion_tokens_details` / `output_tokens_details`)

- **`reasoning_tokens`**: Number of tokens used for internal reasoning (in o-series models)
- **`audio_tokens`**: Number of audio output tokens
- **`accepted_prediction_tokens`**: Tokens from predicted outputs that appeared in completion
- **`rejected_prediction_tokens`**: Tokens from predicted outputs that didn't appear

## Cache Information

### How Caching Works

1. **Automatic Caching**: Prompt caching is enabled by default for supported models
2. **Minimum Threshold**: Caching activates for prompts longer than 1,024 tokens
3. **Granular Caching**: After the first 1,024 tokens, cache hits occur for every 128 additional identical tokens
4. **Cache Sensitivity**: A single character difference in the first 1,024 tokens results in a cache miss

### Cache Detection

- **Cache Hit**: `cached_tokens > 0` in `prompt_tokens_details`
- **Cache Miss**: `cached_tokens == 0` in `prompt_tokens_details`

### Cache Retention

- **Standard Retention**: Cached prefixes remain active for a certain period
- **Extended Retention**: Available for up to 24 hours (released November 2025)
- **Storage Mechanism**: Extended caching offloads key/value tensors to GPU-local storage when memory is full

### Cache Benefits

- **Cost Savings**: 50% discount on cached prompt tokens
- **Performance**: Faster processing times for cached content
- **Efficiency**: Reduced compute requirements for repeated prompts

## Pricing Impact

### Token Pricing Structure

- **Input Tokens**: Lower cost (e.g., GPT-5: $1.25 per million tokens)
- **Output Tokens**: Higher cost (e.g., GPT-5: $10 per million tokens)
- **Cached Tokens**: 50% discount on input token pricing (e.g., GPT-5: $0.125 per million)

### Cost Calculation Example

```json
{
  "usage": {
    "prompt_tokens": 2006,
    "prompt_tokens_details": {
      "cached_tokens": 1920
    },
    "completion_tokens": 300,
    "total_tokens": 2306
  }
}
```

- **Uncached Input**: 86 tokens × full price
- **Cached Input**: 1,920 tokens × 50% discount
- **Output**: 300 tokens × full output price

## Supported Models

### Models with Caching Support

- GPT-4o
- GPT-4o-mini
- GPT-5 series
- o1-preview
- o1-mini
- o3
- o4-mini

### Models with Reasoning Tokens

- o1 series
- o3 series
- o4-mini
- GPT-5 series (with reasoning enabled)

## Streaming Considerations

When using streaming responses:

- Token counts are updated incrementally
- Final usage chunk contains the complete token counts
- If stream is interrupted or cancelled, you may not receive the final usage chunk

## Best Practices

### Maximizing Cache Hits

1. **Keep Prefixes Consistent**: Maintain identical prefixes across requests
2. **Minimize Variations**: Avoid unnecessary changes in the first 1,024 tokens
3. **Structure Prompts**: Place static content at the beginning
4. **Use System Prompts**: Keep system prompts consistent

### Monitoring Token Usage

1. **Track Cache Hit Rate**: Monitor `cached_tokens` vs total `prompt_tokens`
2. **Analyze Reasoning Tokens**: Review `reasoning_tokens` for o-series models
3. **Monitor Audio Tokens**: Track `audio_tokens` for audio processing
4. **Calculate Costs**: Use detailed breakdowns for accurate cost estimation

## API Version Differences

### Chat Completions API (v1/chat/completions)

```json
{
  "prompt_tokens": 1117,
  "completion_tokens": 46,
  "total_tokens": 1163,
  "prompt_tokens_details": {
    "cached_tokens": 0,
    "audio_tokens": 0
  },
  "completion_tokens_details": {
    "reasoning_tokens": 0,
    "audio_tokens": 0,
    "accepted_prediction_tokens": 0,
    "rejected_prediction_tokens": 0
  }
}
```

### Responses API (v1/responses)

```json
{
  "input_tokens": 1289,
  "input_tokens_details": {
    "cached_tokens": 0
  },
  "output_tokens": 685,
  "output_tokens_details": {
    "reasoning_tokens": 640
  },
  "total_tokens": 1974
}
```

## Real-World Examples

### Example 1: High Cache Hit Rate

```json
{
  "usage": {
    "prompt_tokens": 2490,
    "prompt_tokens_details": {
      "cached_tokens": 2432
    },
    "completion_tokens": 128,
    "total_tokens": 2618
  }
}
```

- **Cache Coverage**: 97.67% (2432/2490)
- **Cost Savings**: Significant due to high cache hit rate

### Example 2: Reasoning Model Usage

```json
{
  "usage": {
    "prompt_tokens": 139,
    "prompt_tokens_details": {
      "cached_tokens": 0
    },
    "completion_tokens": 240,
    "completion_tokens_details": {
      "reasoning_tokens": 192
    },
    "total_tokens": 379
  }
}
```

- **Reasoning Tokens**: 192 out of 240 completion tokens (80%)
- **Visible Output**: Only 48 tokens shown to user

### Example 3: Deep Research API

```json
{
  "usage": {
    "input_tokens": 60506,
    "input_tokens_details": {
      "cached_tokens": 0
    },
    "output_tokens": 22883,
    "output_tokens_details": {
      "reasoning_tokens": 20416
    },
    "total_tokens": 83389
  }
}
```

- **High Token Usage**: Deep research models consume many tokens
- **Reasoning Dominance**: Most output tokens are for internal reasoning

## Implementation Notes

### Accessing Token Information

```python
# Python SDK example
response = client.chat.completions.create(
    model="gpt-4o",
    messages=[{"role": "user", "content": "Hello"}]
)

# Access usage information
usage = response.usage
print(f"Total tokens: {usage.total_tokens}")
print(f"Cached tokens: {usage.prompt_tokens_details.cached_tokens}")
print(f"Reasoning tokens: {usage.completion_tokens_details.reasoning_tokens}")
```

### Normalizing Between API Versions

```python
def normalize_usage(usage):
    """Convert Responses API format to Chat Completions format"""
    if "input_tokens" in usage:
        # Responses API format
        return {
            "prompt_tokens": usage["input_tokens"],
            "completion_tokens": usage["output_tokens"],
            "total_tokens": usage["total_tokens"],
            "prompt_tokens_details": usage.get("input_tokens_details", {}),
            "completion_tokens_details": usage.get("output_tokens_details", {})
        }
    return usage  # Already in Chat Completions format
```

## References

- [OpenAI API Reference - Chat Completions](https://developers.openai.com/api/reference/resources/chat/subresources/completions/methods/create/)
- [OpenAI Cookbook - Prompt Caching](https://developers.openai.com/cookbook/examples/prompt_caching_201)
- [Azure OpenAI - Prompt Caching](https://learn.microsoft.com/en-us/azure/foundry/openai/how-to/prompt-caching)
- [OpenAI API Changelog](https://developers.openai.com/api/docs/changelog)

## Summary

OpenAI's API provides comprehensive token usage tracking through detailed breakdowns in the response object. Key features include:

1. **Granular Token Tracking**: Separate counts for input, output, cached, reasoning, and audio tokens
2. **Automatic Caching**: Built-in prompt caching with 50% discount for cached tokens
3. **Reasoning Transparency**: Visibility into internal reasoning token usage for o-series models
4. **Cost Optimization**: Detailed breakdowns enable accurate cost calculation and optimization
5. **Performance Monitoring**: Cache hit rates and token patterns help optimize API usage

Understanding these token details is crucial for:

- Accurate cost estimation and budgeting
- Optimizing prompt structure for better cache hit rates
- Monitoring and debugging token usage patterns
- Making informed decisions about model selection and configuration
