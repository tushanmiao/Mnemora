/**
 * 本地安装流程：Skill / 插件 / 宠物。
 *
 * 设置面板和 Chat 的 Slash 命令共用这里的函数。这些流程带着安全动作
 * （未签名包警告、覆盖确认），一旦复制成两份就会随时间漂移，而漂移的
 * 结果通常是其中一份悄悄少了一道确认——所以只保留这一份实现。
 *
 * 全部只读本地文件选择器，不联网、不接受调用方传入路径：
 * 路径一律来自用户在系统对话框里的当前选择。
 */
import { open } from "@tauri-apps/plugin-dialog";
import { importSkill } from "../../skills/api/skills";
import { installPlugin, type PluginImportKind } from "./plugins";
import { importPetPackage, installPetArchive } from "../../pet/api";

export type InstallMode = "zip" | "directory";

export type InstallOutcome = {
  ok: boolean;
  /** 已安装对象的显示名，用于回显；取消时为空。 */
  name?: string;
  message: string;
  /** 取消选择与真正失败要区分开，调用方可据此决定是否报错。 */
  cancelled?: boolean;
};

const CANCELLED: InstallOutcome = { ok: false, cancelled: true, message: "已取消安装。" };

function errorText(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

/** 选一个本地路径；用户取消返回 null。多选被显式排除。 */
async function pickPath(mode: InstallMode, titles: { dir: string; zip: string }, zipLabel: string) {
  const selected = await open(mode === "directory"
    ? { title: titles.dir, multiple: false, directory: true }
    : { title: titles.zip, multiple: false, directory: false, filters: [{ name: zipLabel, extensions: ["zip"] }] });
  return typeof selected === "string" ? selected : null;
}

export async function pickAndInstallSkill(mode: InstallMode): Promise<InstallOutcome> {
  const path = await pickPath(mode, {
    dir: "选择包含 SKILL.md 的目录",
    zip: "选择 Skill ZIP",
  }, "Skill ZIP");
  if (!path) return CANCELLED;

  try {
    const result = await importSkill(path, mode, false);
    if (result.status === "installed") {
      return { ok: true, name: result.skill.name, message: `技能“${result.skill.name}”已安装。` };
    }
    // alreadyExists：是否替换必须由人决定，不能默认覆盖已有技能。
    if (!window.confirm(`技能“${result.skill.name}”已经安装。是否用所选版本替换？`)) {
      return { ok: false, cancelled: true, message: `已保留原有技能“${result.skill.name}”。` };
    }
    const replaced = await importSkill(path, mode, true);
    return { ok: true, name: replaced.skill.name, message: `技能“${replaced.skill.name}”已替换为所选版本。` };
  } catch (error) {
    return { ok: false, message: `安装技能失败：${errorText(error)}` };
  }
}

export async function pickAndInstallPlugin(mode: InstallMode): Promise<InstallOutcome> {
  const kind: PluginImportKind = mode === "directory" ? "directory" : "zip";
  const path = await pickPath(mode, {
    dir: "选择 Mnemora 插件目录",
    zip: "选择 Mnemora 插件 ZIP",
  }, "ZIP");
  if (!path) return CANCELLED;

  // 签名验证尚未接入可信发布者目录，因此安装前必须让用户明确接受这一点。
  if (!window.confirm("插件签名验证尚未接入可信发布者目录。继续安装会把此包视为未验证代码与配置。请只安装你信任来源的插件。")) {
    return CANCELLED;
  }

  try {
    const summary = await installPlugin(path, kind, false, true);
    return { ok: true, name: summary.name, message: `插件“${summary.name}”已安装但尚未启用。请核对权限后再在插件设置中启用。` };
  } catch (error) {
    const message = errorText(error);
    if (!message.includes("already installed")) {
      return { ok: false, message: `安装插件失败：${message}` };
    }
    if (!window.confirm("该插件已安装。是否保存当前版本作为回滚副本并更新？")) {
      return { ok: false, cancelled: true, message: "已保留原有插件版本。" };
    }
    try {
      const summary = await installPlugin(path, kind, true, true);
      return { ok: true, name: summary.name, message: `插件“${summary.name}”已更新并保持停用；原版本已存为回滚副本。` };
    } catch (retryError) {
      return { ok: false, message: `更新插件失败：${errorText(retryError)}` };
    }
  }
}

/**
 * 宠物的两个 API 自带对话框，因此不走 pickPath。
 * 返回 null 表示用户在对话框里取消。
 */
export async function pickAndInstallPet(mode: InstallMode): Promise<InstallOutcome> {
  try {
    const pets = mode === "directory" ? await importPetPackage() : await installPetArchive();
    if (!pets) return CANCELLED;
    const selected = pets.find((pet) => pet.selected);
    return {
      ok: true,
      name: selected?.displayName,
      message: selected
        ? `宠物“${selected.displayName}”已验证、安装并选中。`
        : "宠物资源包已安装。",
    };
  } catch (error) {
    return { ok: false, message: `安装宠物失败：${errorText(error)}` };
  }
}
