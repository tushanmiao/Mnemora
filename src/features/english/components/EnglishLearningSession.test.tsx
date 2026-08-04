import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { defaultEnglishPlanSettings, type EnglishQueueItem } from "../api/learning";
import EnglishLearningSession from "./EnglishLearningSession";

const reviewItem: EnglishQueueItem = {
  progressId: "progress-1",
  itemId: "item-1",
  state: "review",
  exerciseKind: "spelling",
  dueAt: Date.now(),
  ratingPreviews: [],
  snapshot: {
    dictionaryId: 1,
    entryKey: "obfuscate",
    sourceVersion: "test",
    word: "obfuscate",
    groupId: 1,
    groupName: "test",
    pronunciation: "ˈɒbfʌskeɪt",
    translation: "v. 使模糊；使难以理解",
    example: "",
    exampleTranslation: "",
    britishAudio: "https://example.com/audio.mp3",
    americanAudio: "",
    mnemonic: "",
    rootAffixes: "",
  },
};

describe("EnglishLearningSession active recall", () => {
  it("shows the English word during the new-word learning phase", () => {
    const output = renderSession({ ...reviewItem, state: "new" });

    expect(output).toContain("学习新词");
    expect(output).toContain(reviewItem.snapshot.word);
    expect(output).toContain(reviewItem.snapshot.pronunciation);
    expect(output).toContain("学完了，开始回忆");
  });

  it("hides the English answer before submission when reviewing an old word", () => {
    const output = renderSession(reviewItem);

    expect(output).toContain("使模糊；使难以理解");
    expect(output).toContain("播放发音");
    expect(output).toContain("输入英文单词");
    expect(output).not.toContain(reviewItem.snapshot.word);
    expect(output).not.toContain(reviewItem.snapshot.pronunciation);
    expect(output).not.toContain("标准答案");
  });
});

function renderSession(item: EnglishQueueItem) {
  return renderToStaticMarkup(
    <EnglishLearningSession
      item={item}
      position={0}
      total={1}
      settings={defaultEnglishPlanSettings}
      onBack={vi.fn()}
      onAdvance={vi.fn()}
      onCompleted={vi.fn()}
      onMastered={vi.fn()}
      onArchive={vi.fn()}
    />,
  );
}
