// DOMPurify 的空替身，只给 scripts/ 里的 mermaid 校验脚本用。
//
// mermaid 的 sanitizeText 会无条件走 DOMPurify，而真的 DOMPurify 在没有 DOM 的
// node 环境下退化成一个缺方法的对象。语法校验不需要清洗，这里原样返回即可。
// 注意：这个替身不做任何过滤，绝不能进生产代码路径。
const stub = {
  sanitize: (value) => String(value ?? ""),
  addHook: () => undefined,
  removeHook: () => undefined,
  removeHooks: () => undefined,
  removeAllHooks: () => undefined,
  setConfig: () => undefined,
  clearConfig: () => undefined,
  isSupported: true,
  version: "stub",
};

export default stub;
export const sanitize = stub.sanitize;
export const addHook = stub.addHook;
