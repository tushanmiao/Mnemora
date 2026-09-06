import TurndownService from "turndown";
import { gfm } from "turndown-plugin-gfm";

export function htmlClipboardToMarkdown(html: string) {
  if (html.length > 2 * 1024 * 1024) throw new Error("剪贴板内容超过 2 MiB。");
  const document = new DOMParser().parseFromString(html, "text/html");
  document.querySelectorAll("script,style,iframe,object,embed,form,svg,math,link,meta").forEach((node) => node.remove());
  for (const element of document.body.querySelectorAll("*")) {
    for (const attribute of [...element.attributes]) {
      if (!["href", "src", "alt", "title", "colspan", "rowspan", "start"].includes(attribute.name)) element.removeAttribute(attribute.name);
    }
    for (const name of ["href", "src"]) {
      const value = element.getAttribute(name);
      if (value && !/^(https?:\/\/|mailto:|#)/i.test(value)) element.removeAttribute(name);
    }
  }
  const service = new TurndownService({ headingStyle: "atx", codeBlockStyle: "fenced", bulletListMarker: "-" });
  service.use(gfm);
  service.addRule("merged-table", {
    filter: (node) => node.nodeName === "TABLE" && !!(node as Element).querySelector("[colspan],[rowspan]"),
    replacement: (_content, node) => `\n\n${(node as HTMLElement).outerHTML}\n\n`,
  });
  return service.turndown(document.body);
}

export async function imageBase64(file: File) {
  if (file.size > 8 * 1024 * 1024) throw new Error("图片不能超过 8 MiB。");
  const bytes = new Uint8Array(await file.arrayBuffer());
  let binary = "";
  for (let index = 0; index < bytes.length; index += 8192) binary += String.fromCharCode(...bytes.subarray(index, index + 8192));
  return btoa(binary);
}
