import React, { useRef, useEffect } from 'react';
import { Box, Text, Static } from 'ink';

interface Message {
  role: string;
  content: string;
}

interface ChatProps {
  messages: Message[];
  currentReply: string;
  isLoading: boolean;
}

export default function Chat({ messages, currentReply, isLoading }: ChatProps) {
  const scrollRef = useRef(0);
  useEffect(() => {
    scrollRef.current = messages.length + (currentReply ? 1 : 0) + (isLoading ? 1 : 0);
  }, [messages.length, currentReply, isLoading]);

  // 构建完整消息列表（包含当前正在生成的回复）
  const allMessages = [...messages];
  if (currentReply) allMessages.push({ role: 'assistant', content: currentReply });
  if (isLoading && !currentReply) allMessages.push({ role: 'assistant', content: '...' });

  return (
    <Box flexDirection="column" flexGrow={1} overflowY="hidden">
      <Static items={allMessages.map((msg, i) => {
        switch (msg.role) {
          case 'user':
            return <Box key={i}><Text color="cyan" bold>你: {msg.content}</Text></Box>;
          case 'assistant':
            return <Box key={i}><Text wrap="wrap">{msg.content}</Text></Box>;
          case 'tool_call':
            return <Box key={i}><Text color="yellow">┌─ {msg.content}</Text></Box>;
          case 'tool_result':
            return <Box key={i}><Text dimColor="gray">│  {msg.content}</Text></Box>;
          default:
            return null;
        }
      })} />
    </Box>
  );
}
