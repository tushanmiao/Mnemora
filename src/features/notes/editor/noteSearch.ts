import type { EditorView, Panel, ViewUpdate } from "@codemirror/view";
import { SearchQuery, closeSearchPanel, findNext, findPrevious, getSearchQuery, replaceAll, replaceNext, setSearchQuery } from "@codemirror/search";

// Deliberately bounded regular expressions: character classes, escapes and
// anchors are useful for research text without introducing backtracking trees.
export function safeNoteSearchPattern(pattern: string) {
  if (pattern.length > 128) return false;
  let escaped = false, characterClass = false;
  for (const character of pattern) {
    if (escaped) { if (/[1-9k]/.test(character)) return false; escaped = false; continue; }
    if (character === "\\") { escaped = true; continue; }
    if (character === "[") characterClass = true;
    else if (character === "]") characterClass = false;
    else if (!characterClass && /[()*+?{}|]/.test(character)) return false;
  }
  try { new RegExp(pattern); return !escaped && !characterClass; } catch { return false; }
}

export function createNoteSearchPanel(view: EditorView, text: (value: string) => string = (value) => value): Panel {
  const dom = document.createElement("div"); dom.className = "cm-search note-search";
  const searchInput = document.createElement("input"), replaceInput = document.createElement("input");
  searchInput.setAttribute("main-field", "true");
  searchInput.setAttribute("aria-label", text("查找文本")); replaceInput.setAttribute("aria-label", text("替换文本"));
  const initial = getSearchQuery(view.state);
  searchInput.value = initial.search; replaceInput.value = initial.replace;
  const message = document.createElement("span"); message.setAttribute("role", "status");
  const inputs: HTMLInputElement[] = [], mutations: HTMLButtonElement[] = [];
  const check = (label: string) => {
    const wrapper = document.createElement("label"), input = document.createElement("input");
    input.type = "checkbox"; wrapper.append(input, text(label)); dom.append(wrapper); inputs.push(input); return input;
  };
  dom.append(searchInput, replaceInput);
  const sensitive = check("区分大小写"), whole = check("整词"), regexp = check("正则"), scope = check("仅选区");
  sensitive.checked = initial.caseSensitive;
  let range = { from: view.state.selection.main.from, to: view.state.selection.main.to };
  scope.disabled = range.from === range.to;
  regexp.title = text("正则支持字符类、转义与锚点；不支持重复、分组和分支，最多 128 字符。");
  const sync = () => {
    const allowed = !regexp.checked || safeNoteSearchPattern(searchInput.value);
    message.textContent = allowed ? "" : regexp.title;
    const query = new SearchQuery({ search: allowed ? searchInput.value : "", replace: replaceInput.value,
      caseSensitive: sensitive.checked, wholeWord: whole.checked, regexp: regexp.checked, literal: !regexp.checked,
      test: scope.checked ? (_match, _state, from, to) => from >= range.from && to <= range.to : undefined });
    if (!query.eq(getSearchQuery(view.state))) view.dispatch({ effects: setSearchQuery.of(query) });
    if (allowed && query.search) {
      const cursor = query.getCursor(view.state), limit = 10000;
      let count = 0;
      while (!cursor.next().done && count < limit) count++;
      message.textContent = `${count}${count >= limit ? "+" : ""}`;
    }
  };
  searchInput.oninput = replaceInput.oninput = sync; inputs.forEach((input) => { input.onchange = sync; });
  const button = (label: string, action: (view: EditorView) => boolean, mutates = false) => {
    const element = document.createElement("button"); element.type = "button"; element.textContent = text(label);
    element.onclick = () => { sync(); action(view); };
    dom.append(element); if (mutates) mutations.push(element);
  };
  button("上一个", findPrevious); button("下一个", findNext);
  button("替换", replaceNext, true); button("替换全部", replaceAll, true); button("关闭查找", closeSearchPanel);
  dom.append(message);
  dom.onkeydown = (event) => {
    if (event.isComposing) return;
    if (event.key === "Enter") { event.preventDefault(); sync(); if (event.shiftKey) findPrevious(view); else findNext(view); }
    if (event.key === "Escape") { event.preventDefault(); closeSearchPanel(view); view.focus(); }
  };
  const update = (change?: ViewUpdate) => {
    if (change?.docChanged) {
      range = { from: change.changes.mapPos(range.from, -1), to: change.changes.mapPos(range.to, 1) };
    }
    replaceInput.disabled = view.state.readOnly;
    mutations.forEach((button) => { button.disabled = view.state.readOnly; });
  };
  return { dom, top: true, mount() { update(); searchInput.focus(); searchInput.select(); }, update };
}
