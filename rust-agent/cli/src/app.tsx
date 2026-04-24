import React, { useState, useCallback, useEffect, useRef } from 'react';
import { Box, Text, useApp, useInput } from 'ink';
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

  const abortControllerRef = useRef<AbortController | null>(null);

  // ESC 中断正在进行的对话
  useInput((_input, key) => {
    if (key.escape && abortControllerRef.current) {
      abortControllerRef.current.abort();
      abortControllerRef.current = null;
    }
  });

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
    abortControllerRef.current = new AbortController();
    try {
      for await (const event of sendMessage(input, abortControllerRef.current.signal)) {
        switch (event.event) {
          case 'text_delta':
            setCurrentReply(prev => prev + event.data.content);
            break;
          case 'tool_call': {
            const reply = currentReplyRef.current;
            setCurrentReply('');
            currentReplyRef.current = '';
            if (reply) {
              setMessages(prev => [...prev, { role: 'assistant', content: reply }]);
            }
            setMessages(prev => [...prev, { role: 'tool_call', content: JSON.stringify(event.data) }]);
            break;
          }
          case 'tool_result':
            setMessages(prev => [...prev, { role: 'tool_result', content: event.data.output }]);
            break;
          case 'turn_end': {
            const reply = currentReplyRef.current;
            setCurrentReply('');
            currentReplyRef.current = '';
            if (reply) {
              setMessages(prev => [...prev, { role: 'assistant', content: reply }]);
            }
            const apiCalls = event.data?.api_calls;
            const tokenUsage = event.data?.token_usage;
            let info = apiCalls ? `── 完成，API 调用 ${apiCalls} 次` : '── 完成';
            if (tokenUsage) {
              info += ` │ Token: ${tokenUsage.input_tokens}入/${tokenUsage.output_tokens}出`;
              if (tokenUsage.cache_read_tokens > 0) {
                info += ` (缓存命中 ${tokenUsage.cache_read_tokens})`;
              }
            }
            info += ' ──';
            setMessages(prev => [...prev, { role: 'system', content: info }]);
            break;
          }
          case 'error': {
            setError(event.data.message || '未知错误');
            break;
          }
          case 'done': {
            setCurrentReply('');
            currentReplyRef.current = '';
            break;
          }
        }
      }
    } catch (err) {
      const isAbort = err instanceof Error && err.name === 'AbortError';
      const isTerminated = err instanceof TypeError && String(err).includes('terminated');
      if (isAbort || isTerminated) {
        const reply = currentReplyRef.current;
        if (reply) {
          setMessages(prev => [...prev, { role: 'assistant', content: reply }]);
        }
        setMessages(prev => [...prev, { role: 'system', content: isAbort ? '── 已中断 ──' : '── 连接已断开 ──' }]);
      } else {
        setError(String(err));
      }
    } finally {
      setIsLoading(false);
      setCurrentReply('');
      currentReplyRef.current = '';
      abortControllerRef.current = null;
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
