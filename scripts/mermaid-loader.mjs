// node --import 的入口：注册 mermaid-hooks.mjs 里的解析钩子。
// 单纯 --import 一个含 resolve 导出的模块不会生效，必须走 module.register。
import { register } from "node:module";

register(new URL("./mermaid-hooks.mjs", import.meta.url));
