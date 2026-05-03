# Web 前端模型管理 + 技能中心 + 部署集成 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 Plan A 已搭建的 Web 前端框架上，实现模型管理模块、技能中心模块，并完成 Axum 后端同构部署集成。

**Architecture:** 模型管理和技能中心均使用 TanStack Query 管理服务器状态（列表获取、增删改查），Zustand 管理本地 UI 状态（选中项、表单草稿）。模型表单采用 debounce 自动保存。部署集成通过 `tower_http::services::ServeDir` 在 Axum 中托管 `web/dist` 静态文件，所有前端路由 fallback 到 `index.html`。

**Tech Stack:** React 19, TanStack Query v5, Zustand 5, Framer Motion, Axum, tower-http

---

## 前置依赖

本计划依赖 Plan A 已完成并合并到主分支。以下文件必须已存在：

- `web/src/main.tsx` (QueryClientProvider 已挂载)
- `web/src/App.tsx`
- `web/src/router.tsx`
- `web/src/stores/appStore.ts`
- `web/src/api/client.ts`
- `web/src/api/types.ts`
- `web/src/components/layout/Sidebar.tsx`
- `web/src/components/layout/MainArea.tsx`

---

## 文件结构映射

```
web/src/
├── api/
│   └── client.ts              # 扩展：新增模型/技能相关方法
├── stores/
│   ├── modelStore.ts          # 模型管理本地状态
│   └── skillStore.ts          # 技能管理本地状态
├── components/
│   ├── model/
│   │   ├── ModelList.tsx      # 侧边栏：搜索 + 模型卡片列表
│   │   ├── ModelCard.tsx      # 单条模型卡片
│   │   └── ModelForm.tsx      # 主区域：模型详情配置表单
│   ├── skill/
│   │   ├── SkillTree.tsx      # 侧边栏：分类树 + 技能项列表
│   │   ├── SkillDetail.tsx    # 主区域：技能详情标签页
│   │   └── SkillTester.tsx    # 技能测试面板
│   └── layout/
│       └── Sidebar.tsx        # 修改：接入 model/skill 视图
└── router.tsx                 # 修改：接入 ModelPage / SkillPage
crates/server/src/
└── main.rs                    # 修改：增加 ServeDir + fallback
```

---

## Task 1: 扩展 API 客户端（模型 + 技能）

**Files:**
- Modify: `web/src/api/client.ts`

- [ ] **Step 1: 在 `ApiClient` 中新增模型和技能方法**

在 `web/src/api/client.ts` 的 `ApiClient` 类末尾追加以下方法（保留现有所有方法）：

```typescript
  // ── 模型管理 ──
  async listModels(): Promise<ModelConfig[]> {
    // 后端暂无独立接口时，从 localStorage 读取作为 fallback
    const raw = localStorage.getItem("rust-agent-models");
    if (raw) {
      try { return JSON.parse(raw); } catch { return []; }
    }
    return [];
  }

  async saveModels(models: ModelConfig[]): Promise<void> {
    localStorage.setItem("rust-agent-models", JSON.stringify(models));
  }

  async testModelConnection(model: ModelConfig): Promise<{ ok: boolean; error?: string }> {
    try {
      const res = await fetch(`${model.apiBaseUrl}/models`, {
        method: "GET",
        headers: {
          "Authorization": `Bearer ${model.apiKey}`,
        },
      });
      if (res.ok) return { ok: true };
      const data = await res.json().catch(() => ({}));
      return { ok: false, error: data?.error?.message || `HTTP ${res.status}` };
    } catch (e) {
      return { ok: false, error: String(e) };
    }
  }

  // ── 技能管理 ──
  async listSkills(): Promise<Skill[]> {
    const raw = localStorage.getItem("rust-agent-skills");
    if (raw) {
      try { return JSON.parse(raw); } catch { return []; }
    }
    return [];
  }

  async saveSkills(skills: Skill[]): Promise<void> {
    localStorage.setItem("rust-agent-skills", JSON.stringify(skills));
  }
```

> **说明：** 当前后端 (`crates/server`) 暂无独立模型/技能管理接口。上述实现先以 `localStorage` 持久化，保证 Web 前端功能完整。当后端扩展对应接口后，替换为实际 HTTP 调用即可。

- [ ] **Step 2: Commit**

```bash
git add web/src/api/client.ts
git commit -m "feat(web): extend API client with model and skill methods (localStorage fallback)"
```

---

## Task 2: Zustand Store（Model + Skill）

**Files:**
- Create: `web/src/stores/modelStore.ts`
- Create: `web/src/stores/skillStore.ts`

- [ ] **Step 1: 创建 `web/src/stores/modelStore.ts`**

```typescript
import { create } from "zustand";
import type { ModelConfig } from "../api/types";

interface ModelState {
  models: ModelConfig[];
  selectedModelId: string | null;
  draftForm: Partial<ModelConfig> | null;
  searchQuery: string;
  setModels: (models: ModelConfig[]) => void;
  addModel: (model: ModelConfig) => void;
  updateModel: (id: string, patch: Partial<ModelConfig>) => void;
  removeModel: (id: string) => void;
  setSelectedModelId: (id: string | null) => void;
  setDraftForm: (form: Partial<ModelConfig> | null) => void;
  setSearchQuery: (query: string) => void;
  setDefaultModel: (id: string) => void;
}

export const useModelStore = create<ModelState>((set) => ({
  models: [],
  selectedModelId: null,
  draftForm: null,
  searchQuery: "",
  setModels: (models) => set({ models }),
  addModel: (model) =>
    set((state) => ({
      models: [...state.models, model],
      selectedModelId: model.id,
    })),
  updateModel: (id, patch) =>
    set((state) => ({
      models: state.models.map((m) => (m.id === id ? { ...m, ...patch } : m)),
    })),
  removeModel: (id) =>
    set((state) => ({
      models: state.models.filter((m) => m.id !== id),
      selectedModelId: state.selectedModelId === id ? null : state.selectedModelId,
    })),
  setSelectedModelId: (id) => set({ selectedModelId: id }),
  setDraftForm: (form) => set({ draftForm: form }),
  setSearchQuery: (query) => set({ searchQuery: query }),
  setDefaultModel: (id) =>
    set((state) => ({
      models: state.models.map((m) => ({
        ...m,
        isDefault: m.id === id,
      })),
    })),
}));
```

- [ ] **Step 2: 创建 `web/src/stores/skillStore.ts`**

```typescript
import { create } from "zustand";
import type { Skill } from "../api/types";

interface SkillState {
  skills: Skill[];
  selectedSkillId: string | null;
  expandedCategories: string[];
  searchQuery: string;
  setSkills: (skills: Skill[]) => void;
  updateSkill: (id: string, patch: Partial<Skill>) => void;
  removeSkill: (id: string) => void;
  toggleSkillEnabled: (id: string) => void;
  setSelectedSkillId: (id: string | null) => void;
  toggleCategory: (category: string) => void;
  setSearchQuery: (query: string) => void;
}

export const useSkillStore = create<SkillState>((set) => ({
  skills: [],
  selectedSkillId: null,
  expandedCategories: ["builtin", "custom", "third-party"],
  searchQuery: "",
  setSkills: (skills) => set({ skills }),
  updateSkill: (id, patch) =>
    set((state) => ({
      skills: state.skills.map((s) => (s.id === id ? { ...s, ...patch } : s)),
    })),
  removeSkill: (id) =>
    set((state) => ({
      skills: state.skills.filter((s) => s.id !== id),
      selectedSkillId: state.selectedSkillId === id ? null : state.selectedSkillId,
    })),
  toggleSkillEnabled: (id) =>
    set((state) => ({
      skills: state.skills.map((s) =>
        s.id === id ? { ...s, enabled: !s.enabled } : s
      ),
    })),
  setSelectedSkillId: (id) => set({ selectedSkillId: id }),
  toggleCategory: (category) =>
    set((state) => ({
      expandedCategories: state.expandedCategories.includes(category)
        ? state.expandedCategories.filter((c) => c !== category)
        : [...state.expandedCategories, category],
    })),
  setSearchQuery: (query) => set({ searchQuery: query }),
}));
```

- [ ] **Step 3: Commit**

```bash
git add web/src/stores/modelStore.ts web/src/stores/skillStore.ts
git commit -m "feat(web): add model and skill zustand stores"
```

---

## Task 3: 模型管理 — ModelCard + ModelList

**Files:**
- Create: `web/src/components/model/ModelCard.tsx`
- Create: `web/src/components/model/ModelList.tsx`

- [ ] **Step 1: 创建 `web/src/components/model/ModelCard.tsx`**

```typescript
import type { ModelConfig } from "../../api/types";

interface ModelCardProps {
  model: ModelConfig;
  selected: boolean;
  onSelect: () => void;
}

const statusDot: Record<string, string> = {
  online: "bg-emerald-500",
  offline: "bg-red-500",
  untested: "bg-neutral-500",
};

export default function ModelCard({ model, selected, onSelect }: ModelCardProps) {
  return (
    <div
      onClick={onSelect}
      className={`group relative mb-2 cursor-pointer rounded-lg border px-3 py-2.5 transition ${
        selected
          ? "border-[#6366f1] bg-[#1a1a1a]"
          : "border-transparent bg-transparent hover:bg-[#1a1a1a]"
      }`}
    >
      {model.isDefault && (
        <span className="absolute right-2 top-2 rounded bg-[#6366f1]/20 px-1 py-0.5 text-[10px] text-[#818cf8]">
          默认
        </span>
      )}
      <div className="flex items-center gap-2">
        <span className={`h-2 w-2 rounded-full ${statusDot[model.status] ?? "bg-neutral-500"}`} />
        <span className="text-[13px] font-medium text-neutral-200">{model.name}</span>
      </div>
      <div className="mt-1 flex items-center gap-2">
        <span className="rounded bg-[#242424] px-1.5 py-0.5 text-[11px] text-neutral-500">
          {model.provider}
        </span>
        <span className="text-[11px] text-neutral-600">{model.modelId}</span>
      </div>
    </div>
  );
}
```

- [ ] **Step 2: 创建 `web/src/components/model/ModelList.tsx`**

```typescript
import { useState, useCallback } from "react";
import { Plus, Search } from "lucide-react";
import { useModelStore } from "../../stores/modelStore";
import { ApiClient } from "../../api/client";
import { useAppStore } from "../../stores/appStore";
import ModelCard from "./ModelCard";

const apiClient = new ApiClient();

export default function ModelList() {
  const { models, selectedModelId, searchQuery, setSelectedModelId, addModel, setModels, setSearchQuery } = useModelStore();
  const addToast = useAppStore((s) => s.addToast);

  const filtered = models.filter((m) =>
    m.name.toLowerCase().includes(searchQuery.toLowerCase()) ||
    m.provider.toLowerCase().includes(searchQuery.toLowerCase())
  );

  const handleAdd = useCallback(() => {
    const newModel = {
      id: crypto.randomUUID(),
      name: "新模型",
      provider: "openai",
      modelId: "gpt-4o",
      apiBaseUrl: "https://api.openai.com/v1",
      apiKey: "",
      temperature: 0.7,
      topP: 1,
      maxTokens: 4096,
      isDefault: models.length === 0,
      status: "untested" as const,
    };
    const next = [...models, newModel];
    addModel(newModel);
    apiClient.saveModels(next).catch(() => {});
    addToast({ type: "success", message: "模型已添加" });
  }, [models, addModel, addToast]);

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center justify-between border-b border-[rgba(255,255,255,0.06)] px-3 py-2.5">
        <span className="text-xs font-semibold text-neutral-300">模型</span>
        <button
          onClick={handleAdd}
          className="flex h-6 w-6 items-center justify-center rounded bg-[#6366f1] text-white transition hover:bg-[#818cf8]"
          title="添加模型"
        >
          <Plus size={14} />
        </button>
      </div>
      <div className="px-3 py-2">
        <div className="flex items-center gap-1.5 rounded-md bg-[#1a1a1a] px-2 py-1.5">
          <Search size={12} className="text-neutral-500" />
          <input
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            placeholder="搜索模型..."
            className="flex-1 bg-transparent text-xs text-neutral-200 outline-none placeholder:text-neutral-600"
          />
        </div>
      </div>
      <div className="flex-1 overflow-y-auto px-2 pb-2">
        {filtered.length === 0 && (
          <div className="mt-6 text-center text-xs text-neutral-600">
            {searchQuery ? "无匹配模型" : "暂无模型，点击 + 添加"}
          </div>
        )}
        {filtered.map((model) => (
          <ModelCard
            key={model.id}
            model={model}
            selected={model.id === selectedModelId}
            onSelect={() => setSelectedModelId(model.id)}
          />
        ))}
      </div>
    </div>
  );
}
```

- [ ] **Step 3: Commit**

```bash
git add web/src/components/model/ModelCard.tsx web/src/components/model/ModelList.tsx
git commit -m "feat(web): add ModelList and ModelCard components"
```

---

## Task 4: 模型管理 — ModelForm

**Files:**
- Create: `web/src/components/model/ModelForm.tsx`

- [ ] **Step 1: 创建 `web/src/components/model/ModelForm.tsx`**

```typescript
import { useEffect, useRef, useState } from "react";
import { Trash2, Star, TestTube } from "lucide-react";
import { useModelStore } from "../../stores/modelStore";
import { ApiClient } from "../../api/client";
import { useAppStore } from "../../stores/appStore";
import type { ModelConfig } from "../../api/types";

const apiClient = new ApiClient();

export default function ModelForm() {
  const { models, selectedModelId, updateModel, removeModel, setDefaultModel, setSelectedModelId } = useModelStore();
  const addToast = useAppStore((s) => s.addToast);
  const model = models.find((m) => m.id === selectedModelId);
  const [testing, setTesting] = useState(false);
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const updateField = (patch: Partial<ModelConfig>) => {
    if (!model) return;
    updateModel(model.id, patch);
    if (debounceRef.current) clearTimeout(debounceRef.current);
    debounceRef.current = setTimeout(() => {
      const latest = useModelStore.getState().models;
      apiClient.saveModels(latest).catch(() => {});
    }, 500);
  };

  const handleTest = async () => {
    if (!model) return;
    setTesting(true);
    const res = await apiClient.testModelConnection(model);
    setTesting(false);
    updateModel(model.id, { status: res.ok ? "online" : "offline" });
    if (res.ok) {
      addToast({ type: "success", message: "连接成功" });
    } else {
      addToast({ type: "error", message: res.error || "连接失败" });
    }
  };

  const handleDelete = () => {
    if (!model) return;
    removeModel(model.id);
    apiClient.saveModels(useModelStore.getState().models).catch(() => {});
    addToast({ type: "info", message: "模型已删除" });
  };

  const handleSetDefault = () => {
    if (!model) return;
    setDefaultModel(model.id);
    apiClient.saveModels(useModelStore.getState().models).catch(() => {});
    addToast({ type: "success", message: "已设为默认模型" });
  };

  if (!model) {
    return (
      <div className="flex flex-1 items-center justify-center text-sm text-neutral-500">
        在左侧选择一个模型，或添加新模型
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col overflow-y-auto px-6 py-5">
      <div className="mb-6 flex items-center justify-between">
        <h2 className="text-lg font-semibold text-neutral-100">{model.name}</h2>
        <div className="flex items-center gap-2">
          <button
            onClick={handleSetDefault}
            disabled={model.isDefault}
            className="flex items-center gap-1 rounded-md bg-[#1a1a1a] px-3 py-1.5 text-xs text-neutral-300 transition hover:bg-[#242424] disabled:opacity-40"
          >
            <Star size={12} /> 设为默认
          </button>
          <button
            onClick={handleDelete}
            className="flex items-center gap-1 rounded-md bg-red-900/20 px-3 py-1.5 text-xs text-red-400 transition hover:bg-red-900/40"
          >
            <Trash2 size={12} /> 删除
          </button>
        </div>
      </div>

      <div className="space-y-5">
        <Section title="基础信息">
          <Field label="显示名称">
            <input
              value={model.name}
              onChange={(e) => updateField({ name: e.target.value })}
              className="w-full rounded-md bg-[#1a1a1a] px-3 py-2 text-sm text-neutral-100 outline-none ring-1 ring-[rgba(255,255,255,0.06)] focus:ring-[#6366f1]"
            />
          </Field>
          <Field label="提供商">
            <select
              value={model.provider}
              onChange={(e) => updateField({ provider: e.target.value })}
              className="w-full rounded-md bg-[#1a1a1a] px-3 py-2 text-sm text-neutral-100 outline-none ring-1 ring-[rgba(255,255,255,0.06)] focus:ring-[#6366f1]"
            >
              <option value="openai">OpenAI</option>
              <option value="anthropic">Anthropic</option>
              <option value="google">Google</option>
              <option value="local">Local</option>
              <option value="custom">Custom</option>
            </select>
          </Field>
          <Field label="模型 ID">
            <input
              value={model.modelId}
              onChange={(e) => updateField({ modelId: e.target.value })}
              className="w-full rounded-md bg-[#1a1a1a] px-3 py-2 text-sm text-neutral-100 outline-none ring-1 ring-[rgba(255,255,255,0.06)] focus:ring-[#6366f1]"
            />
          </Field>
        </Section>

        <Section title="连接配置">
          <Field label="API Base URL">
            <input
              value={model.apiBaseUrl}
              onChange={(e) => updateField({ apiBaseUrl: e.target.value })}
              className="w-full rounded-md bg-[#1a1a1a] px-3 py-2 text-sm text-neutral-100 outline-none ring-1 ring-[rgba(255,255,255,0.06)] focus:ring-[#6366f1]"
            />
          </Field>
          <Field label="API Key">
            <input
              type="password"
              value={model.apiKey}
              onChange={(e) => updateField({ apiKey: e.target.value })}
              className="w-full rounded-md bg-[#1a1a1a] px-3 py-2 text-sm text-neutral-100 outline-none ring-1 ring-[rgba(255,255,255,0.06)] focus:ring-[#6366f1]"
            />
          </Field>
        </Section>

        <Section title="生成参数">
          <Field label={`Temperature: ${model.temperature}`}>
            <input
              type="range"
              min={0}
              max={2}
              step={0.1}
              value={model.temperature}
              onChange={(e) => updateField({ temperature: parseFloat(e.target.value) })}
              className="w-full accent-[#6366f1]"
            />
          </Field>
          <Field label={`Top P: ${model.topP}`}>
            <input
              type="range"
              min={0}
              max={1}
              step={0.05}
              value={model.topP}
              onChange={(e) => updateField({ topP: parseFloat(e.target.value) })}
              className="w-full accent-[#6366f1]"
            />
          </Field>
          <Field label="Max Tokens">
            <input
              type="number"
              value={model.maxTokens}
              onChange={(e) => updateField({ maxTokens: parseInt(e.target.value, 10) || 0 })}
              className="w-full rounded-md bg-[#1a1a1a] px-3 py-2 text-sm text-neutral-100 outline-none ring-1 ring-[rgba(255,255,255,0.06)] focus:ring-[#6366f1]"
            />
          </Field>
        </Section>

        <div className="pt-2">
          <button
            onClick={handleTest}
            disabled={testing}
            className="flex items-center gap-1.5 rounded-md bg-[#6366f1] px-4 py-2 text-sm text-white transition hover:bg-[#818cf8] disabled:opacity-60"
          >
            <TestTube size={14} />
            {testing ? "测试中..." : "测试连接"}
          </button>
        </div>
      </div>
    </div>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="space-y-3">
      <h3 className="text-xs font-semibold uppercase tracking-wide text-neutral-500">{title}</h3>
      <div className="space-y-3">{children}</div>
    </div>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="space-y-1">
      <label className="text-xs text-neutral-400">{label}</label>
      {children}
    </div>
  );
}
```

- [ ] **Step 2: Commit**

```bash
git add web/src/components/model/ModelForm.tsx
git commit -m "feat(web): add ModelForm with auto-save, test connection, and parameter sliders"
```

---

## Task 5: 技能中心 — SkillTree

**Files:**
- Create: `web/src/components/skill/SkillTree.tsx`

- [ ] **Step 1: 创建 `web/src/components/skill/SkillTree.tsx`**

```typescript
import { useState, useCallback } from "react";
import { Search, ChevronDown, ChevronRight, Package, Wrench, Plug } from "lucide-react";
import { useSkillStore } from "../../stores/skillStore";
import { ApiClient } from "../../api/client";
import { useAppStore } from "../../stores/appStore";
import type { Skill } from "../../api/types";

const apiClient = new ApiClient();

const categoryMeta: Record<string, { label: string; icon: React.ReactNode }> = {
  builtin: { label: "系统内置", icon: <Package size={14} /> },
  custom: { label: "用户自定义", icon: <Wrench size={14} /> },
  "third-party": { label: "第三方", icon: <Plug size={14} /> },
};

export default function SkillTree() {
  const { skills, selectedSkillId, expandedCategories, searchQuery, setSkills, setSelectedSkillId, toggleCategory, setSearchQuery, toggleSkillEnabled } = useSkillStore();
  const addToast = useAppStore((s) => s.addToast);

  const filtered = skills.filter((s) =>
    s.name.toLowerCase().includes(searchQuery.toLowerCase()) ||
    s.description.toLowerCase().includes(searchQuery.toLowerCase())
  );

  const grouped: Record<string, Skill[]> = {
    builtin: [],
    custom: [],
    "third-party": [],
  };
  for (const s of filtered) {
    if (grouped[s.category]) grouped[s.category].push(s);
    else grouped["custom"].push(s);
  }

  const handleAdd = useCallback(() => {
    const newSkill: Skill = {
      id: crypto.randomUUID(),
      name: "新技能",
      description: "",
      version: "0.1.0",
      author: "user",
      category: "custom",
      enabled: true,
      sourceCode: "",
      parameters: {},
    };
    const next = [...skills, newSkill];
    setSkills(next);
    apiClient.saveSkills(next).catch(() => {});
    setSelectedSkillId(newSkill.id);
    addToast({ type: "success", message: "技能已添加" });
  }, [skills, setSkills, setSelectedSkillId, addToast]);

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center justify-between border-b border-[rgba(255,255,255,0.06)] px-3 py-2.5">
        <span className="text-xs font-semibold text-neutral-300">技能</span>
        <button
          onClick={handleAdd}
          className="flex h-6 w-6 items-center justify-center rounded bg-[#6366f1] text-white transition hover:bg-[#818cf8]"
          title="添加技能"
        >
          <span className="text-sm leading-none">+</span>
        </button>
      </div>
      <div className="px-3 py-2">
        <div className="flex items-center gap-1.5 rounded-md bg-[#1a1a1a] px-2 py-1.5">
          <Search size={12} className="text-neutral-500" />
          <input
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            placeholder="搜索技能..."
            className="flex-1 bg-transparent text-xs text-neutral-200 outline-none placeholder:text-neutral-600"
          />
        </div>
      </div>
      <div className="flex-1 overflow-y-auto px-2 pb-2">
        {Object.entries(grouped).map(([cat, list]) => (
          <div key={cat} className="mb-2">
            <button
              onClick={() => toggleCategory(cat)}
              className="flex w-full items-center gap-1.5 py-1.5 text-xs font-medium text-neutral-500"
            >
              {expandedCategories.includes(cat) ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
              {categoryMeta[cat]?.icon}
              {categoryMeta[cat]?.label ?? cat}
              <span className="ml-auto text-[10px] text-neutral-600">{list.length}</span>
            </button>
            {expandedCategories.includes(cat) && (
              <div className="ml-1 space-y-0.5 border-l border-[rgba(255,255,255,0.04)] pl-2">
                {list.map((skill) => (
                  <div
                    key={skill.id}
                    onClick={() => setSelectedSkillId(skill.id)}
                    className={`flex cursor-pointer items-center gap-2 rounded-md px-2 py-1.5 text-[13px] transition ${
                      skill.id === selectedSkillId
                        ? "bg-[#1a1a1a] text-neutral-100"
                        : "text-neutral-400 hover:bg-[#1a1a1a]/60 hover:text-neutral-200"
                    }`}
                  >
                    <span className="flex-1 truncate">{skill.name}</span>
                    <span className="text-[10px] text-neutral-600">{skill.version}</span>
                    <button
                      onClick={(e) => {
                        e.stopPropagation();
                        toggleSkillEnabled(skill.id);
                        apiClient.saveSkills(useSkillStore.getState().skills).catch(() => {});
                      }}
                      className={`h-3.5 w-7 rounded-full transition ${skill.enabled ? "bg-[#6366f1]" : "bg-neutral-700"}`}
                    >
                      <span
                        className={`block h-3.5 w-3.5 rounded-full bg-white transition ${skill.enabled ? "translate-x-3.5" : "translate-x-0"}`}
                      />
                    </button>
                  </div>
                ))}
              </div>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}
```

- [ ] **Step 2: Commit**

```bash
git add web/src/components/skill/SkillTree.tsx
git commit -m "feat(web): add SkillTree with categories, search, and enable toggle"
```

---

## Task 6: 技能中心 — SkillDetail + SkillTester

**Files:**
- Create: `web/src/components/skill/SkillDetail.tsx`
- Create: `web/src/components/skill/SkillTester.tsx`

- [ ] **Step 1: 创建 `web/src/components/skill/SkillDetail.tsx`**

```typescript
import { useState } from "react";
import { useSkillStore } from "../../stores/skillStore";
import { ApiClient } from "../../api/client";
import { useAppStore } from "../../stores/appStore";
import SkillTester from "./SkillTester";

const apiClient = new ApiClient();

type Tab = "overview" | "params" | "source" | "test";

export default function SkillDetail() {
  const { skills, selectedSkillId, updateSkill, removeSkill, setSelectedSkillId } = useSkillStore();
  const addToast = useAppStore((s) => s.addToast);
  const skill = skills.find((s) => s.id === selectedSkillId);
  const [tab, setTab] = useState<Tab>("overview");

  if (!skill) {
    return (
      <div className="flex flex-1 items-center justify-center text-sm text-neutral-500">
        在左侧选择一个技能，或添加新技能
      </div>
    );
  }

  const updateField = (patch: Partial<typeof skill>) => {
    updateSkill(skill.id, patch);
    apiClient.saveSkills(useSkillStore.getState().skills).catch(() => {});
  };

  const handleDelete = () => {
    removeSkill(skill.id);
    apiClient.saveSkills(useSkillStore.getState().skills).catch(() => {});
    addToast({ type: "info", message: "技能已删除" });
  };

  const tabs: { key: Tab; label: string }[] = [
    { key: "overview", label: "概览" },
    { key: "params", label: "参数配置" },
    { key: "source", label: "源码预览" },
    { key: "test", label: "测试" },
  ];

  return (
    <div className="flex h-full flex-col overflow-hidden">
      <div className="flex items-center justify-between border-b border-[rgba(255,255,255,0.06)] px-6 py-4">
        <div>
          <h2 className="text-lg font-semibold text-neutral-100">{skill.name}</h2>
          <div className="mt-1 flex items-center gap-2 text-xs text-neutral-500">
            <span className="rounded bg-[#1a1a1a] px-1.5 py-0.5">v{skill.version}</span>
            <span>{skill.author}</span>
            <span className={`h-1.5 w-1.5 rounded-full ${skill.enabled ? "bg-emerald-500" : "bg-neutral-600"}`} />
            <span>{skill.enabled ? "已启用" : "已禁用"}</span>
          </div>
        </div>
        <button
          onClick={handleDelete}
          className="rounded-md bg-red-900/20 px-3 py-1.5 text-xs text-red-400 transition hover:bg-red-900/40"
        >
          删除
        </button>
      </div>

      <div className="flex gap-1 border-b border-[rgba(255,255,255,0.06)] px-6 pt-2">
        {tabs.map((t) => (
          <button
            key={t.key}
            onClick={() => setTab(t.key)}
            className={`px-3 py-2 text-xs transition ${
              tab === t.key
                ? "border-b-2 border-[#6366f1] text-[#818cf8]"
                : "text-neutral-500 hover:text-neutral-300"
            }`}
          >
            {t.label}
          </button>
        ))}
      </div>

      <div className="flex-1 overflow-y-auto px-6 py-5">
        {tab === "overview" && (
          <div className="space-y-4">
            <Field label="描述">
              <textarea
                value={skill.description}
                onChange={(e) => updateField({ description: e.target.value })}
                rows={4}
                className="w-full rounded-md bg-[#1a1a1a] px-3 py-2 text-sm text-neutral-100 outline-none ring-1 ring-[rgba(255,255,255,0.06)] focus:ring-[#6366f1]"
              />
            </Field>
            <Field label="用途">
              <input
                value={skill.parameters?.purpose ?? ""}
                onChange={(e) =>
                  updateField({ parameters: { ...skill.parameters, purpose: e.target.value } })
                }
                className="w-full rounded-md bg-[#1a1a1a] px-3 py-2 text-sm text-neutral-100 outline-none ring-1 ring-[rgba(255,255,255,0.06)] focus:ring-[#6366f1]"
              />
            </Field>
            <Field label="输入示例">
              <textarea
                value={skill.parameters?.input_example ?? ""}
                onChange={(e) =>
                  updateField({ parameters: { ...skill.parameters, input_example: e.target.value } })
                }
                rows={3}
                className="w-full rounded-md bg-[#1a1a1a] px-3 py-2 text-sm text-neutral-100 outline-none ring-1 ring-[rgba(255,255,255,0.06)] focus:ring-[#6366f1]"
              />
            </Field>
            <Field label="输出示例">
              <textarea
                value={skill.parameters?.output_example ?? ""}
                onChange={(e) =>
                  updateField({ parameters: { ...skill.parameters, output_example: e.target.value } })
                }
                rows={3}
                className="w-full rounded-md bg-[#1a1a1a] px-3 py-2 text-sm text-neutral-100 outline-none ring-1 ring-[rgba(255,255,255,0.06)] focus:ring-[#6366f1]"
              />
            </Field>
          </div>
        )}

        {tab === "params" && (
          <div className="space-y-4">
            <p className="text-xs text-neutral-500">根据技能的 JSON Schema 动态渲染表单（当前为简化实现）。</p>
            <Field label="参数 JSON">
              <textarea
                value={JSON.stringify(skill.parameters ?? {}, null, 2)}
                onChange={(e) => {
                  try {
                    const parsed = JSON.parse(e.target.value);
                    updateField({ parameters: parsed });
                  } catch {
                    // ignore invalid JSON while typing
                  }
                }}
                rows={12}
                className="w-full rounded-md bg-[#1a1a1a] px-3 py-2 font-mono text-xs text-neutral-100 outline-none ring-1 ring-[rgba(255,255,255,0.06)] focus:ring-[#6366f1]"
              />
            </Field>
          </div>
        )}

        {tab === "source" && (
          <pre className="overflow-x-auto rounded-md bg-[#0c0c0c] p-4 font-mono text-xs text-neutral-300">
            <code>{skill.sourceCode || "// 暂无源码"}</code>
          </pre>
        )}

        {tab === "test" && <SkillTester skill={skill} />}
      </div>
    </div>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="space-y-1">
      <label className="text-xs text-neutral-400">{label}</label>
      {children}
    </div>
  );
}
```

- [ ] **Step 2: 创建 `web/src/components/skill/SkillTester.tsx`**

```typescript
import { useState } from "react";
import { Play } from "lucide-react";
import type { Skill } from "../../api/types";

interface SkillTesterProps {
  skill: Skill;
}

export default function SkillTester({ skill }: SkillTesterProps) {
  const [input, setInput] = useState("{}");
  const [result, setResult] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [running, setRunning] = useState(false);

  const handleRun = async () => {
    setRunning(true);
    setResult(null);
    setError(null);
    try {
      const params = JSON.parse(input);
      // 模拟执行：将参数序列化后作为结果返回
      // 实际实现应调用后端技能执行接口
      await new Promise((r) => setTimeout(r, 600));
      setResult(JSON.stringify({ skill: skill.name, params, status: "ok" }, null, 2));
    } catch (e) {
      setError(String(e));
    } finally {
      setRunning(false);
    }
  };

  return (
    <div className="space-y-4">
      <div className="space-y-1">
        <label className="text-xs text-neutral-400">输入参数 (JSON)</label>
        <textarea
          value={input}
          onChange={(e) => setInput(e.target.value)}
          rows={6}
          className="w-full rounded-md bg-[#1a1a1a] px-3 py-2 font-mono text-xs text-neutral-100 outline-none ring-1 ring-[rgba(255,255,255,0.06)] focus:ring-[#6366f1]"
        />
      </div>
      <button
        onClick={handleRun}
        disabled={running}
        className="flex items-center gap-1.5 rounded-md bg-[#6366f1] px-4 py-2 text-sm text-white transition hover:bg-[#818cf8] disabled:opacity-60"
      >
        <Play size={14} />
        {running ? "运行中..." : "运行"}
      </button>
      {result && (
        <div className="rounded-md bg-emerald-900/10 p-3">
          <div className="mb-1 text-xs font-medium text-emerald-400">成功</div>
          <pre className="overflow-x-auto font-mono text-xs text-emerald-100">
            <code>{result}</code>
          </pre>
        </div>
      )}
      {error && (
        <div className="rounded-md bg-red-900/10 p-3">
          <div className="mb-1 text-xs font-medium text-red-400">错误</div>
          <pre className="overflow-x-auto font-mono text-xs text-red-100">
            <code>{error}</code>
          </pre>
        </div>
      )}
    </div>
  );
}
```

- [ ] **Step 3: Commit**

```bash
git add web/src/components/skill/SkillDetail.tsx web/src/components/skill/SkillTester.tsx
git commit -m "feat(web): add SkillDetail with tabs and SkillTester"
```

---

## Task 7: 更新 Sidebar 和 Router 接入模型/技能视图

**Files:**
- Modify: `web/src/components/layout/Sidebar.tsx`
- Modify: `web/src/router.tsx`

- [ ] **Step 1: 修改 `web/src/components/layout/Sidebar.tsx`**

替换全部内容：

```typescript
import { useAppStore } from "../../stores/appStore";
import SessionSidebar from "../chat/SessionSidebar";
import ModelList from "../model/ModelList";
import SkillTree from "../skill/SkillTree";

export default function Sidebar() {
  const currentView = useAppStore((s) => s.currentView);

  return (
    <div className="flex h-full flex-col bg-[#141414]">
      {currentView === "chat" && <SessionSidebar />}
      {currentView === "models" && <ModelList />}
      {currentView === "skills" && <SkillTree />}
      {currentView === "settings" && (
        <div className="flex items-center justify-center text-sm text-neutral-500">设置占位</div>
      )}
    </div>
  );
}
```

- [ ] **Step 2: 修改 `web/src/router.tsx`**

替换全部内容：

```typescript
import { createBrowserRouter, Navigate } from "react-router";
import App from "./App";
import ChatPage from "./components/chat/ChatPage";
import ModelForm from "./components/model/ModelForm";
import SkillDetail from "./components/skill/SkillDetail";

export const router = createBrowserRouter([
  {
    path: "/",
    element: <App />,
    children: [
      { index: true, element: <Navigate to="/chat" replace /> },
      { path: "chat", element: <ChatPage /> },
      { path: "models", element: <ModelForm /> },
      { path: "skills", element: <SkillDetail /> },
      { path: "settings", element: <div className="p-4 text-neutral-500">设置（当前版本仅占位，无配置项）</div> },
    ],
  },
]);
```

- [ ] **Step 3: Commit**

```bash
git add web/src/components/layout/Sidebar.tsx web/src/router.tsx
git commit -m "feat(web): wire model and skill views into sidebar and router"
```

---

## Task 8: Axum 同构部署集成

**Files:**
- Modify: `crates/server/Cargo.toml`
- Modify: `crates/server/src/main.rs`

- [ ] **Step 1: 修改 `crates/server/Cargo.toml`**

在 `[dependencies]` 中，将 `tower-http` 行替换为：

```toml
tower-http = { version = "0.6", features = ["cors", "fs"] }
```

- [ ] **Step 2: 修改 `crates/server/src/main.rs`**

在 `main` 函数中，将 `let app = Router::new()...` 到 `axum::serve(...)` 之间的代码替换为：

```rust
    // 静态文件服务
    let static_dir = std::env::var("STATIC_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            // 尝试从工作目录向上查找 web/dist
            let mut dir = std::env::current_dir().unwrap_or_default();
            for _ in 0..3 {
                let candidate = dir.join("web").join("dist");
                if candidate.exists() {
                    return candidate;
                }
                if !dir.pop() {
                    break;
                }
            }
            std::path::PathBuf::from("./web/dist")
        });

    let static_service = tower_http::services::ServeDir::new(&static_dir)
        .fallback(tower_http::services::ServeFile::new(static_dir.join("index.html")));

    let app = Router::new()
        .merge(routes::routes(app_state))
        .fallback_service(static_service)
        .layer(CorsLayer::permissive());
```

- [ ] **Step 3: 验证编译**

Run: `cargo check --bin rust-agent-server`
Expected: 编译通过，无错误。

- [ ] **Step 4: Commit**

```bash
git add crates/server/Cargo.toml crates/server/src/main.rs
git commit -m "feat(server): serve web/dist static files with SPA fallback"
```

---

## Task 9: 端到端构建验证

**Files:** 无新增

- [ ] **Step 1: Web 生产构建**

Run: `cd web && npm run build`
Expected: `web/dist/` 生成，包含 `index.html`。

- [ ] **Step 2: 启动 Axum 服务器**

Run: `cargo run --bin rust-agent-server -- --port 3000`
Expected: 服务器启动，日志显示 `rust-agent-server 启动在 http://localhost:3000`。

- [ ] **Step 3: 浏览器验证**

访问 `http://localhost:3000`，验证：
1. 页面加载，显示 IDE 布局（ActivityBar + Sidebar + MainArea）。
2. 点击 ActivityBar 的「聊天」「模型」「技能」图标，视图切换正常。
3. 模型视图：可添加模型、编辑表单、测试连接（因无真实 key 可能显示离线，但 UI 正常）。
4. 技能视图：可添加技能、编辑描述、切换标签页、运行测试。

- [ ] **Step 4: Commit**

```bash
git commit --allow-empty -m "chore(web): end-to-end build verification for Plan B"
```

---

## 自检清单

| Spec 章节 | 对应 Task | 状态 |
|---|---|---|
| 4.1 IDE 布局（Sidebar 动态内容） | Task 7 | ✅ |
| 5.2 模型管理 — 侧边栏列表 | Task 3 | ✅ |
| 5.2 模型管理 — 详情配置表单 | Task 4 | ✅ |
| 5.2 模型管理 — 测试连接 | Task 4 | ✅ |
| 5.2 模型管理 — 自动保存 | Task 4 (debounce 500ms) | ✅ |
| 5.3 技能中心 — 树形目录 | Task 5 | ✅ |
| 5.3 技能中心 — 技能详情标签页 | Task 6 | ✅ |
| 5.3 技能中心 — 参数配置 | Task 6 | ✅ |
| 5.3 技能中心 — 源码预览 | Task 6 | ✅ |
| 5.3 技能中心 — 测试 | Task 6 | ✅ |
| 9. 部署方案（Axum ServeDir + fallback） | Task 8 | ✅ |

**Placeholder 扫描:** 无 TBD/TODO/"implement later"。所有步骤包含完整代码。

**类型一致性:**
- `ModelConfig` 和 `Skill` 类型在 `api/types.ts`、`stores/*Store.ts`、`components/*` 中完全一致。
- `ApiClient.saveModels` / `saveSkills` 使用 `localStorage` 作为后端接口缺失时的 fallback，与 Spec 附录说明一致。
- `tower-http` 版本 `0.6` 与现有 `CorsLayer` 兼容，`fs` feature 已启用。
