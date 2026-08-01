import { Channel, invoke, isTauri } from "@tauri-apps/api/core";
import type { SignedUpdateInfo, UpdateCheckResult, UpdateDownloadProgress } from "../../../types/appUpdate";

export function checkApplicationUpdate() {
  if (!isTauri()) throw new Error("版本检查需要在 Tauri 应用中运行。");
  return invoke<UpdateCheckResult>("check_application_update");
}

export function checkSignedApplicationUpdate() {
  if (!isTauri()) throw new Error("签名更新需要在 Tauri 应用中运行。");
  return invoke<SignedUpdateInfo | null>("check_signed_application_update");
}

export function downloadAndInstallSignedUpdate(
  onProgress: (progress: UpdateDownloadProgress) => void,
) {
  if (!isTauri()) throw new Error("签名更新需要在 Tauri 应用中运行。");
  const channel = new Channel<UpdateDownloadProgress>();
  channel.onmessage = onProgress;
  return invoke<void>("download_and_install_application_update", { onProgress: channel });
}

export function discardSignedApplicationUpdate() {
  if (!isTauri()) return Promise.resolve();
  return invoke<void>("discard_signed_application_update");
}
