#!/usr/bin/env node
import { spawn } from "child_process";
import { resolve, dirname } from "path";
import { fileURLToPath } from "url";

const __dirname = dirname(fileURLToPath(import.meta.url));

// 用 node 直接运行 tsx 的入口，避免 Windows spawn .cmd 的 EINVAL 问题
const tsxEntry = resolve(__dirname, "../node_modules/tsx/dist/cli.mjs");
const index = resolve(__dirname, "../src/index.tsx");

// 如果没有 RUST_AGENT_ROOT，自动推导为 cli 的上两级（项目根目录）
if (!process.env.RUST_AGENT_ROOT) {
  process.env.RUST_AGENT_ROOT = resolve(__dirname, "../..");
}

const child = spawn(process.execPath, [tsxEntry, index, ...process.argv.slice(2)], {
  stdio: "inherit",
  env: process.env,
});

child.on("exit", (code) => process.exit(code ?? 0));
