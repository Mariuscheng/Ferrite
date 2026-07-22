export interface ChatMsg { role: string; content: string; }

// Known model lists per provider (DeepSeek v4 API updated 2026)
export const MODEL_LISTS: Record<string, string[]> = {
    openai: ['gpt-4.1', 'gpt-4.1-mini', 'gpt-4o', 'gpt-4o-mini', 'o3-mini', 'o1', 'o1-mini'],
    anthropic: ['claude-sonnet-4-20250514', 'claude-3-5-haiku-20241022', 'claude-3-5-sonnet-20241022', 'claude-3-opus-20240229'],
    deepseek: ['deepseek-v4-pro', 'deepseek-v4-flash', 'deepseek-chat', 'deepseek-reasoner'],
    ollama: ['codellama', 'llama3', 'deepseek-coder', 'qwen2.5-coder', 'mistral'],
};

export const ENDPOINT_PRESETS: Record<string, string> = {
    openai: 'https://api.openai.com/v1',
    anthropic: 'https://api.anthropic.com/v1',
    ollama: 'http://localhost:11434',
    deepseek: 'https://api.deepseek.com/',
};