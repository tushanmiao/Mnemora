import { invoke } from "@tauri-apps/api/core";

export interface NetworkConnectionProbe {
  id: "search" | "page";
  ok: boolean;
  statusCode: number | null;
  durationMs: number;
  message: string;
}

export interface NetworkConnectionReport {
  proxyMode: string;
  proxySource: string;
  proxyAddress: string | null;
  probes: NetworkConnectionProbe[];
}

export function testWebNetworkConnection() {
  return invoke<NetworkConnectionReport>("test_web_network_connection");
}
