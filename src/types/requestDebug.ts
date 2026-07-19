import type { ModelUsage } from "./chat";

export interface RequestDebugRequest {
  method: string;
  url: string;
  headers: Record<string, string>;
  body: unknown;
  bodyTruncated: boolean;
  stream: boolean;
}

export interface RequestDebugResponse {
  statusCode?: number;
  body?: unknown;
  bodyTruncated: boolean;
}

export interface RequestDebugRecord {
  id: string;
  createdAtMs: number;
  durationMs: number;
  providerId: string;
  providerName: string;
  modelId: string;
  apiModel: string;
  displayName: string;
  protocol: string;
  status: "success" | "error" | "stopped";
  conversationId?: string;
  messageId?: string;
  request: RequestDebugRequest;
  response: RequestDebugResponse;
  usage?: ModelUsage;
}
