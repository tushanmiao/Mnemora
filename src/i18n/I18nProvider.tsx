import { createContext, useContext, useEffect, useMemo, useState, type ReactNode } from "react";
import type { InterfaceLanguage } from "../types/appSettings";
import { type TranslationKey, zhTranslations } from "./translations";

type TranslationValues = Record<string, string | number>;
type Translate = (key: TranslationKey, values?: TranslationValues) => string;

const I18nContext = createContext<{ language: InterfaceLanguage; t: Translate }>({
  language: "zh",
  t: (key, values) => interpolate(zhTranslations[key], values),
});

function interpolate(template: string, values?: TranslationValues) {
  if (!values) return template;
  return template.replace(/\{(\w+)\}/g, (match, key) => (
    values[key] === undefined ? match : String(values[key])
  ));
}

export function I18nProvider({ language, children }: { language: InterfaceLanguage; children: ReactNode }) {
  const [english, setEnglish] = useState<Record<TranslationKey, string> | null>(null);

  useEffect(() => {
    document.documentElement.lang = language === "en" ? "en-US" : "zh-CN";
  }, [language]);

  useEffect(() => {
    if (language !== "en" || english) return;
    void import("./english").then(({ enTranslations }) => {
      setEnglish(enTranslations);
    });
  }, [english, language]);

  const value = useMemo(() => ({
    language,
    t: ((key, values) => interpolate(language === "en" && english ? english[key] : zhTranslations[key], values)) as Translate,
  }), [english, language]);

  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

export function useI18n() {
  return useContext(I18nContext);
}
