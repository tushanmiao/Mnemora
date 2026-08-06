import { Archive, ArrowLeft, Award, Check, Headphones, Lightbulb, LoaderCircle, Play, Volume2 } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import {
  resolveEnglishAudio,
  submitEnglishAttempt,
  type EnglishAttemptResult,
  type EnglishPlanSettings,
  type EnglishQueueItem,
  type EnglishRating,
  type EnglishVerdict,
} from "../api/learning";
import { judgeEnglishAnswer, suggestEnglishRating } from "../utils/answerNormalization";
import { createManagedAudio, type ManagedAudio } from "../../../runtime/media/managedAudio";

type Props = {
  item: EnglishQueueItem;
  position: number;
  total: number;
  settings: EnglishPlanSettings;
  onBack: () => void;
  onAdvance: () => void;
  onCompleted: (result: EnglishAttemptResult) => void;
  onMastered: () => Promise<void>;
  onArchive: () => Promise<void>;
};

const ratingLabels: Record<EnglishRating, string> = { again: "忘记", hard: "困难", good: "记得", easy: "简单" };
const verdictLabels: Record<EnglishVerdict, string> = { correct: "正确", acceptable: "可接受", incorrect: "需要复习", skipped: "未作答" };

export default function EnglishLearningSession({ item, position, total, settings, onBack, onAdvance, onCompleted, onMastered, onArchive }: Props) {
  const [intro, setIntro] = useState(item.state === "new");
  const [answer, setAnswer] = useState("");
  const [hintLevel, setHintLevel] = useState(0);
  const [hintCount, setHintCount] = useState(0);
  const [revealed, setRevealed] = useState(false);
  const [verdict, setVerdict] = useState<EnglishVerdict | null>(null);
  const [suggested, setSuggested] = useState<EnglishRating | null>(null);
  const [result, setResult] = useState<EnglishAttemptResult | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const inputRef = useRef<HTMLInputElement | null>(null);
  const audioRef = useRef<ManagedAudio | null>(null);
  const audioRequestRef = useRef(0);
  const composingRef = useRef(false);
  const submittedAnswerRef = useRef("");
  const startedAt = useRef(Date.now());
  const attemptId = useRef(createAttemptId());
  const snapshot = item.snapshot;

  const audioUrl = settings.preferredAccent === "british"
    ? snapshot.britishAudio || snapshot.americanAudio
    : snapshot.americanAudio || snapshot.britishAudio;

  const playAudio = async (rate = settings.playbackRate) => {
    if (!audioUrl) return;
    const request = ++audioRequestRef.current;
    audioRef.current?.release();
    let playableUrl = audioUrl;
    try {
      playableUrl = await resolveEnglishAudio(audioUrl);
    } catch {
      playableUrl = audioUrl;
    }
    if (request !== audioRequestRef.current) return;
    const managed = createManagedAudio(playableUrl, `english-session:${item.progressId}`);
    const audio = managed.audio;
    audio.playbackRate = rate;
    audioRef.current = managed;
    void audio.play().catch(() => setError("音频暂时无法播放，可以继续使用非音频提示。"));
  };

  useEffect(() => {
    startedAt.current = Date.now();
    attemptId.current = createAttemptId();
    setIntro(item.state === "new");
    setAnswer("");
    submittedAnswerRef.current = "";
    setHintLevel(0);
    setHintCount(0);
    setRevealed(false);
    setVerdict(null);
    setSuggested(null);
    setResult(null);
    setError("");
    if (settings.autoPlay && (item.exerciseKind === "dictation" || item.state === "new")) {
      window.setTimeout(() => playAudio(), 80);
    }
    return () => {
      audioRequestRef.current += 1;
      audioRef.current?.release();
      audioRef.current = null;
    };
  }, [item.progressId]);

  useEffect(() => {
    if (!intro && !revealed) inputRef.current?.focus();
  }, [intro, revealed]);

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (busy || result) return;
      if (event.altKey && event.key.toLowerCase() === "p") {
        event.preventDefault();
        playAudio();
        return;
      }
      if (event.altKey && event.key.toLowerCase() === "h" && !revealed && !intro) {
        event.preventDefault();
        requestHint();
        return;
      }
      if (intro && event.key === "Enter") {
        event.preventDefault();
        startPractice();
        return;
      }
      if (!revealed || event.ctrlKey || event.altKey || event.metaKey) return;
      const ratings: Record<string, EnglishRating> = { "1": "again", "2": "hard", "3": "good", "4": "easy" };
      const rating = ratings[event.key];
      if (rating) {
        event.preventDefault();
        void rate(rating);
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [busy, intro, revealed, result, answer, hintLevel, hintCount, item.progressId]);

  const partialWord = useMemo(() => snapshot.word.split("").map((character, index) => index % 2 === 0 || character === "-" || character === "'" ? character : "_").join(" "), [snapshot.word]);

  const startPractice = () => {
    setIntro(false);
    startedAt.current = Date.now();
  };

  const reveal = (hintOverride = hintLevel) => {
    const submittedAnswer = inputRef.current?.value ?? answer;
    submittedAnswerRef.current = submittedAnswer;
    setAnswer(submittedAnswer);
    const responseMs = Math.max(0, Date.now() - startedAt.current);
    const nextVerdict = judgeEnglishAnswer(item.exerciseKind, submittedAnswer, snapshot.word, snapshot.translation);
    setVerdict(nextVerdict);
    setSuggested(suggestEnglishRating(nextVerdict, hintOverride, responseMs));
    setRevealed(true);
  };

  const requestHint = () => {
    const nextLevel = Math.min(5, hintLevel + 1);
    setHintLevel(nextLevel);
    setHintCount((count) => count + 1);
    if (nextLevel === 1 && audioUrl) playAudio(Math.max(0.6, settings.playbackRate - 0.2));
    if (nextLevel === 5) reveal(nextLevel);
  };

  const rate = async (rating: EnglishRating) => {
    if (busy || !revealed) return;
    setBusy(true);
    setError("");
    try {
      const next = await submitEnglishAttempt({
        attemptId: attemptId.current,
        progressId: item.progressId,
        exerciseKind: item.exerciseKind,
        rawAnswer: submittedAnswerRef.current,
        hintLevel,
        hintCount,
        responseMs: Math.max(0, Date.now() - startedAt.current),
        finalRating: rating,
      });
      setResult(next);
      onCompleted(next);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setBusy(false);
    }
  };

  const handleLifecycle = async (action: () => Promise<void>) => {
    setBusy(true);
    setError("");
    try {
      await action();
      onAdvance();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="english-session">
      <header className="english-session-header">
        <button className="english-icon-button" type="button" onClick={onBack} title="结束本组" aria-label="结束本组"><ArrowLeft size={17} /></button>
        <div><strong>{exerciseLabel(item.exerciseKind)}</strong><span>{position + 1} / {total}</span></div>
        <div className="english-session-actions">
          <button className="english-icon-button" type="button" disabled={busy} onClick={() => void handleLifecycle(onMastered)} title="标为已掌握" aria-label="标为已掌握"><Award size={16} /></button>
          <button className="english-icon-button" type="button" disabled={busy} onClick={() => {
            if (window.confirm(`确定归档“${snapshot.word}”吗？之后可以在“进度 → 已归档单词”中恢复。`)) {
              void handleLifecycle(onArchive);
            }
          }} title="归档单词" aria-label="归档单词"><Archive size={16} /></button>
        </div>
      </header>

      <div className="english-session-progress"><i style={{ width: `${Math.round((position + (result ? 1 : 0)) / Math.max(1, total) * 100)}%` }} /></div>

      <main className="english-exercise-stage">
        {intro ? (
          <section className="english-word-intro">
            <span className="english-state-label">学习新词</span>
            <h2>{snapshot.word}</h2>
            <p className="english-pronunciation">/{snapshot.pronunciation}/</p>
            <p className="english-intro-translation">{snapshot.translation}</p>
            {snapshot.example ? <blockquote><p>{snapshot.example}</p><small>{snapshot.exampleTranslation}</small></blockquote> : null}
            {snapshot.rootAffixes || snapshot.mnemonic ? <p className="english-intro-note">{snapshot.rootAffixes || snapshot.mnemonic}</p> : null}
            <div className="english-intro-actions">
              {audioUrl ? <button type="button" className="english-secondary-button" onClick={() => void playAudio()}><Volume2 size={16} />播放发音</button> : null}
              <button type="button" onClick={startPractice}><Play size={16} />学完了，开始回忆</button>
            </div>
          </section>
        ) : (
          <section className="english-review-card">
            <div className="english-prompt-area">
              {item.exerciseKind === "meaning_recall" ? <><span>回忆这个单词的含义</span><h2>{snapshot.word}</h2><p>/{snapshot.pronunciation}/</p></> : null}
              {item.exerciseKind === "spelling" ? <><span>根据中文释义回忆并拼写英文</span><h3>{snapshot.translation}</h3>{audioUrl ? <button type="button" className="english-recall-audio" onClick={() => void playAudio()}><Volume2 size={17} />播放发音</button> : null}</> : null}
              {item.exerciseKind === "dictation" ? <><span>听音拼写</span><button type="button" className="english-audio-primary" onClick={() => void playAudio()} disabled={!audioUrl}><Headphones size={26} /><small>{audioUrl ? "播放发音" : "音频不可用"}</small></button></> : null}
            </div>

            <form className="english-answer-form" onSubmit={(event) => { event.preventDefault(); if (!revealed && !composingRef.current) reveal(); }}>
              <input
                ref={inputRef}
                value={answer}
                disabled={revealed || busy}
                maxLength={500}
                autoComplete="off"
                spellCheck={false}
                onChange={(event) => setAnswer(event.target.value)}
                onCompositionStart={() => { composingRef.current = true; }}
                onCompositionEnd={(event) => { composingRef.current = false; setAnswer(event.currentTarget.value); }}
                onKeyDown={(event) => { if (event.key === "Enter" && (composingRef.current || event.nativeEvent.isComposing)) event.preventDefault(); }}
                placeholder={item.exerciseKind === "meaning_recall" ? "输入你回忆的释义" : "输入英文单词"}
              />
              <button type="submit" disabled={revealed || busy}><Check size={17} />检查答案</button>
            </form>

            <div className="english-hints" aria-live="polite">
              {hintLevel >= 2 ? <p>首字母：<strong>{snapshot.word.slice(0, 1)}</strong> · {snapshot.word.length} 个字符</p> : null}
              {hintLevel >= 3 && item.exerciseKind !== "spelling" ? <p>中文：<strong>{snapshot.translation}</strong></p> : null}
              {hintLevel >= 3 && item.exerciseKind === "spelling" && snapshot.exampleTranslation ? <p>例句语境：<strong>{snapshot.exampleTranslation}</strong></p> : null}
              {hintLevel >= 4 ? <p className="english-partial-word">{partialWord}</p> : null}
              <button type="button" disabled={hintLevel >= 5 || revealed} onClick={requestHint}><Lightbulb size={16} />{hintLevel === 0 ? "使用提示" : `继续提示 ${hintLevel}/5`}</button>
            </div>

            {revealed ? (
              <div className="english-answer-result">
                <div className={`english-verdict is-${verdict}`}><strong>{verdict ? verdictLabels[verdict] : ""}</strong><span>标准答案：{item.exerciseKind === "meaning_recall" ? snapshot.translation : snapshot.word}</span></div>
                {!result ? <><p>系统建议：<strong>{suggested ? ratingLabels[suggested] : ""}</strong>。最终评级由你决定。</p><div className="english-rating-grid">{(["again", "hard", "good", "easy"] as EnglishRating[]).map((rating) => {
                  const preview = item.ratingPreviews.find((itemPreview) => itemPreview.rating === rating);
                  return <button key={rating} type="button" className={suggested === rating ? "is-suggested" : ""} disabled={busy} onClick={() => void rate(rating)}><strong>{ratingLabels[rating]}</strong><small>{formatInterval(preview?.scheduledDays, preview?.dueAt)}</small></button>;
                })}</div></> : <div className="english-session-complete"><p>已按“{ratingLabels[result.finalRating]}”保存，下次复习：{new Date(result.nextDueAt).toLocaleString()}</p><button type="button" onClick={onAdvance}>{position + 1 >= total ? "完成本组" : "下一个单词"}</button></div>}
              </div>
            ) : null}
          </section>
        )}
      </main>
      {busy ? <div className="english-session-busy" role="status"><LoaderCircle className="english-spinner" size={16} />正在保存</div> : null}
      {error ? <p className="english-error" role="alert">{error}</p> : null}
    </div>
  );
}


function exerciseLabel(kind: EnglishQueueItem["exerciseKind"]) {
  return ({ meaning_recall: "释义回忆", spelling: "释义拼写", dictation: "听音拼写" } as const)[kind];
}

function formatInterval(days?: number, dueAt?: number) {
  if (days && days > 0) return `${days} 天`;
  if (!dueAt) return "稍后";
  const minutes = Math.max(1, Math.round((dueAt - Date.now()) / 60_000));
  return `${minutes} 分钟`;
}

function createAttemptId() {
  return globalThis.crypto?.randomUUID?.() ?? `${Date.now()}-${Math.random().toString(36).slice(2)}`;
}
