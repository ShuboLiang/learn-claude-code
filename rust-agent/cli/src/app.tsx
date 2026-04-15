import React, { useState, useCallback, useEffect, useRef } from 'react';
import { Box, Text, useApp } from 'ink';
import { sendMessage, init, createSession, clearSession } from './api';
import Chat from './chat';
import Input from './input';

export default function App({ serverUrl }: { serverUrl: string }) {
  const { exit } = useApp();
  const [sessionId, setSessionId] = useState('');
  const [model, setModel] = useState('');
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [messages, setMessages] = useState<Array<{ role: string; content: string }>>([]);
  const [currentReply, setCurrentReply] = useState('');

  // 使用 ref 追踪 currentReply，避免 stale closure
  const currentReplyRef = useRef(currentReply);
  currentReplyRef.current = currentReply;

  useEffect(() => {
    (async () => {
      init(serverUrl, '');
      try {
        const { id, model } = await createSession();
        setSessionId(id);
        setModel(model);
      } catch (err) {
        setError(`会话创建失败: ${err}`);
      }
    })();
  }, [serverUrl]);

  const handleSubmit = useCallback(async (input: string) => {
    if (!input.trim() || isLoading || !sessionId) return;
    setError(null);
    setMessages(prev => [...prev, { role: 'user', content: input }]);
    setIsLoading(true);
    setCurrentReply('');
    try {
      for await (const event of sendMessage(input)) {
        switch (event.event) {
          case 'text_delta':
            setCurrentReply(prev => prev + event.data.content);
            break;
          case 'tool_call': {
            const reply = currentReplyRef.current;
            if (reply) {
              setMessages(prev => [...prev, { role: 'assistant', content: reply }]);
              setCurrentReply('');
            }
            setMessages(prev => [...prev, { role: 'tool_call', content: JSON.stringify(event.data) }]);
            break;
          }
          case 'tool_result':
            setMessages(prev => [...prev, { role: 'tool_result', content: event.data.output }]);
            break;
          case 'turn_end': {
            const reply = currentReplyRef.current;
            if (reply) {
              setMessages(prev => [...prev, { role: 'assistant', content: reply }]);
              setCurrentReply('');
            }
            const apiCalls = event.data?.api_calls;
            if (apiCalls) {
              setMessages(prev => [...prev, { role: 'system', content: `── 完成，API 调用 ${apiCalls} 次 ──` }]);
            }
            break;
          }
          case 'done': {
            const reply = currentReplyRef.current;
            if (reply) {
              setMessages(prev => [...prev, { role: 'assistant', content: reply }]);
              setCurrentReply('');
            }
            break;
          }
        }
      }
    } catch (err) {
      setError(String(err));
    } finally {
      setIsLoading(false);
    }
  }, [sessionId, isLoading]);

  const handleQuit = useCallback((input: string) => {
    const lower = input.trim().toLowerCase();
    if (lower === 'q' || lower === 'quit' || lower === 'exit' || lower === '/exit') exit();
  }, [exit]);

  const handleClear = useCallback(() => {
    setMessages(prev => [...prev, { role: 'system', content: '═══ 以上对话已被清除 ═══' }]);
    setCurrentReply('');
    clearSession().catch(() => {});
  }, []);

  return (
    <Box flexDirection="column" height="100%">
      <Chat messages={messages} currentReply={currentReply} isLoading={isLoading} />
      <Input onSubmit={handleSubmit} onQuit={handleQuit} onClear={handleClear} isLoading={isLoading} model={model} />
      {error && <Box><Text color="red">Error: {error}</Text></Box>}
    </Box>
  );
}
