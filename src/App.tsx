import "./styles/app.css";
import { ChatHeader } from "./components/ChatHeader";
import { ChatInput } from "./components/ChatInput";
import { MessageList } from "./components/MessageList";
import { Sidebar } from "./components/Sidebar";

function App() {
  return (
    <main className="app-shell" aria-label="Mnemora application">
      <Sidebar />

      <section className="chat-workspace" aria-label="当前对话">
        <ChatHeader />
        <MessageList />
        <ChatInput />
      </section>
    </main>
  );
}

export default App;
