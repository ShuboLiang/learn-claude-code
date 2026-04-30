import { test } from "node:test";
import assert from "node:assert";
import { transformMessages } from "../src/session-utils.js";

test("user text message", () => {
  const result = transformMessages([{ role: "user", content: "hello" }]);
  assert.deepStrictEqual(result, [{ role: "user", content: "hello" }]);
});

test("assistant text message", () => {
  const result = transformMessages([{ role: "assistant", content: "hi" }]);
  assert.deepStrictEqual(result, [{ role: "assistant", content: "hi" }]);
});

test("assistant with tool_use", () => {
  const result = transformMessages([
    {
      role: "assistant",
      content: [
        { type: "text", text: "Let me search." },
        {
          type: "tool_use",
          name: "search",
          input: { q: "test" },
          id: "t1",
        },
      ],
    },
  ]);
  assert.deepStrictEqual(result, [
    { role: "assistant", content: "Let me search." },
    {
      role: "tool_call",
      content: JSON.stringify({ name: "search", input: { q: "test" } }),
    },
  ]);
});

test("user with tool_result", () => {
  const result = transformMessages([
    {
      role: "user",
      content: [
        { type: "text", text: "Result:" },
        { type: "tool_result", tool_use_id: "t1", content: "found" },
      ],
    },
  ]);
  assert.deepStrictEqual(result, [
    { role: "user", content: "Result:" },
    { role: "tool_result", content: "found" },
  ]);
});

test("skips unknown role", () => {
  const result = transformMessages([{ role: "system", content: "warn" }]);
  assert.deepStrictEqual(result, []);
});
