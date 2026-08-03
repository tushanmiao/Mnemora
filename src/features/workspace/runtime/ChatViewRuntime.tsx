import { createContext, useContext, type ReactNode } from "react";

const ChatViewRuntimeContext = createContext<ReactNode | null>(null);

export function ChatViewRuntimeProvider({
  chatPanel,
  children,
}: {
  chatPanel: ReactNode;
  children: ReactNode;
}) {
  return (
    <ChatViewRuntimeContext.Provider value={chatPanel}>
      {children}
    </ChatViewRuntimeContext.Provider>
  );
}

export function useChatViewRuntime() {
  const chatPanel = useContext(ChatViewRuntimeContext);
  if (!chatPanel) throw new Error("Chat 视图运行时尚未初始化。");
  return chatPanel;
}
