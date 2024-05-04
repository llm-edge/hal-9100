// https://docs.mistral.ai/platform/endpoints/
export type MistralChatModelId =
  | 'HF://mlc-ai/Llama-3-8B-Instruct-q4f16_1-MLC'
  | (string & {});

export interface MistralChatSettings {
  /**
Whether to inject a safety prompt before all conversations.

Defaults to `false`.
   */
  safePrompt?: boolean;
}