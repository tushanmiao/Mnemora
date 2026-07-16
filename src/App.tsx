import { useState } from "react";
import "./styles/app.css";
import { ChatHeader } from "./components/ChatHeader";
import { ChatInput } from "./components/ChatInput";
import { MessageList } from "./components/MessageList";
import { Sidebar } from "./components/Sidebar";

function App() {
  const [theme, setTheme] = useState<"light" | "dark">("light");

  return (
    <main className="app-shell" data-theme={theme} aria-label="Mnemora application">
      <Sidebar />

      <section className="chat-workspace" aria-label="当前对话">
        <ChatHeader theme={theme} onToggleTheme={() => setTheme(theme === "light" ? "dark" : "light")} />
        <MessageList />
        <ChatInput />
      </section>
    </main>
  );
}

export default App;
