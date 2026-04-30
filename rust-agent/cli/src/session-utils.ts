export interface Message {
  role: string;
  content: string;
}

export function transformMessages(
  apiMessages: Array<{ role: string; content: any }>,
): Message[] {
  const result: Message[] = [];

  for (const msg of apiMessages) {
    if (msg.role === "user") {
      if (typeof msg.content === "string") {
        result.push({ role: "user", content: msg.content });
      } else if (Array.isArray(msg.content)) {
        let texts: string[] = [];
        for (const block of msg.content) {
          if (block?.type === "tool_result") {
            if (texts.length > 0) {
              result.push({ role: "user", content: texts.join("") });
              texts = [];
            }
            result.push({
              role: "tool_result",
              content: String(block.content ?? ""),
            });
          } else if (block?.type === "text" && typeof block.text === "string") {
            texts.push(block.text);
          }
        }
        if (texts.length > 0) {
          result.push({ role: "user", content: texts.join("") });
        }
      }
    } else if (msg.role === "assistant") {
      if (typeof msg.content === "string") {
        result.push({ role: "assistant", content: msg.content });
      } else if (Array.isArray(msg.content)) {
        for (const block of msg.content) {
          if (block?.type === "text" && typeof block.text === "string") {
            result.push({ role: "assistant", content: block.text });
          } else if (block?.type === "tool_use") {
            result.push({
              role: "tool_call",
              content: JSON.stringify({
                name: block.name,
                input: block.input,
              }),
            });
          }
        }
      }
    } else {
      console.warn(`Unknown role in history: ${msg.role}`);
    }
  }

  return result;
}
