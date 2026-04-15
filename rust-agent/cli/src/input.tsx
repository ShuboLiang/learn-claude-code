import React, { useState } from 'react';
import { Box, Text } from 'ink';
import TextInput from 'ink-text-input';

interface InputProps {
  onSubmit: (text: string) => void;
  onQuit: (text: string) => void;
  isLoading: boolean;
}

export default function Input({ onSubmit, onQuit, isLoading }: InputProps) {
  const [value, setValue] = useState('');

  const handleSubmit = (text: string) => {
    if (!text.trim()) return;
    const lower = text.trim().toLowerCase();
    if (lower === 'q' || lower === 'quit' || lower === 'exit') {
      onQuit(text);
      return;
    }
    setValue('');
    onSubmit(text);
  };

  return (
    <Box borderStyle="single" paddingLeft={1}>
      <TextInput
        value={value}
        onChange={setValue}
        onSubmit={handleSubmit}
        placeholder={isLoading ? '(等待响应中...)' : '输入消息，Enter 提交...'}
      />
    </Box>
  );
}
