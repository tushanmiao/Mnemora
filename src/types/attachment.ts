/** 已复制到会话独立目录、可以随消息持久化的附件。 */
export interface ChatAttachment {
  id: string;
  kind: "image" | "file";
  name: string;
  mimeType: string;
  sizeBytes: number;
  /** 会话附件目录中的相对文件名。 */
  path: string;
  /** 会话附件目录中的缩略图相对文件名；旧会话可能没有。 */
  previewPath?: string;
  width?: number;
  height?: number;
}

/** 尚未发送的本地文件或剪贴板临时文件。 */
export interface PendingChatAttachment {
  id: string;
  kind: "image" | "file";
  name: string;
  mimeType: string;
  sizeBytes: number;
  /** 文件选择器返回的路径，或 Rust 创建的剪贴板临时文件路径。 */
  path: string;
  previewPath?: string;
  width?: number;
  height?: number;
}
