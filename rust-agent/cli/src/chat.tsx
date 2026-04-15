import React from 'react';
import { Box, Text, Static } from 'ink';
import Markdown from './markdown';

interface Message {
  role: string;
  content: string;
}

interface ChatProps {
  messages: Message[];
  currentReply: string;
  isLoading: boolean;
}

// 解析 tool_call JSON，提取工具名和简短描述
function formatToolCall(content: string): { name: string; desc: string } {
  try {
    const data = JSON.parse(content);
    const name = data.name || 'unknown';
    const input = data.input || {};
    // 提取简短描述
    const desc = input.command || input.path || input.query || input.content
      ? String(input.command || input.path || input.query || input.content || '').slice(0, 60)
      : JSON.stringify(input).slice(0, 60);
    return { name, desc };
  } catch {
    return { name: 'tool', desc: content.slice(0, 60) };
  }
}

// 渲染单条消息
function renderMessage(msg: Message, index: number) {
  switch (msg.role) {
    case 'user':
      return (
        <Box key={`msg-${index}`}>
          <Text color="cyan" bold>你: {msg.content}</Text>
        </Box>
      );
    case 'assistant':
      return (
        <Box key={`msg-${index}`} flexDirection="column">
          <Markdown content={msg.content} />
        </Box>
      );
    case 'tool_call': {
      const { name, desc } = formatToolCall(msg.content);
      return (
        <Box key={`msg-${index}`} flexDirection="column">
          <Text color="yellow">┌─ 🔧 {name}</Text>
          <Text color="yellow" dimColor>│  {desc}</Text>
        </Box>
      );
    }
    case 'tool_result': {
      const maxShow = 5000;
      const text = msg.content.length > maxShow
        ? msg.content.slice(0, maxShow) + `\n... [还有 ${msg.content.length - maxShow} 字符未显示]`
        : msg.content;
      return (
        <Box key={`msg-${index}`} flexDirection="column">
          <Text dimColor>└─ {text}</Text>
        </Box>
      );
    }
    case 'system':
      return (
        <Box key={`msg-${index}`}>
          <Text dimColor>{msg.content}</Text>
        </Box>
      );
    default:
      return <Box key={`msg-${index}`} />;
  }
}

export default function Chat({ messages, currentReply, isLoading }: ChatProps) {
  return (
    <Box flexDirection="column" flexGrow={1}>
      {/* 已完成的消息用 Static 永久渲染 */}
      <Static items={messages}>
        {(msg, index) => renderMessage(msg, index)}
      </Static>

      {/* 当前正在生成的回复（纯文本实时显示，避免 ANSI 码导致 Ink 高度计算偏差） */}
      {currentReply && (
        <Box>
          <Text wrap="wrap">{currentReply}</Text>
        </Box>
      )}

      {/* 加载指示器（实时内容，不在 Static 中） */}
      {isLoading && !currentReply && (
        <Box>
          <Text dimColor>思考中...</Text>
        </Box>
      )}
    </Box>
  );
}
