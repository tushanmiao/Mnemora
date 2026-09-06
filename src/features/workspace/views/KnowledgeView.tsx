import { KnowledgeCenter } from "../../knowledge/components/KnowledgeCenter";
import { useKnowledgeViewRuntime } from "../runtime/KnowledgeViewRuntime";

export default function KnowledgeView() {
  const runtime = useKnowledgeViewRuntime();
  return <KnowledgeCenter {...runtime} />;
}
