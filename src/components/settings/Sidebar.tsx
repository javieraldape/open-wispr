import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { getVersion } from "@tauri-apps/api/app";
import type { AppSettings, ModelInfo } from "@/bindings";
import { getTranslatedModelName } from "@/lib/utils/modelTranslation";
import { startWindowDrag } from "@/lib/utils/windowDrag";
import { useModelStore } from "@/stores/modelStore";
import { BrandMark } from "../brand/OpenWisprBrand";
import { SECTIONS_CONFIG, type SettingsSection } from "./sections";

interface SettingsSidebarProps {
  activeSection: SettingsSection;
  onSectionChange: (section: SettingsSection) => void;
  settings: AppSettings | null;
}

const getCurrentModelInfo = (
  models: ModelInfo[],
  currentModel: string,
): ModelInfo | undefined => models.find((model) => model.id === currentModel);

const SettingsSidebar: React.FC<SettingsSidebarProps> = ({
  activeSection,
  onSectionChange,
  settings,
}) => {
  const { t } = useTranslation();
  const [version, setVersion] = useState("");
  const { models, currentModel, loading } = useModelStore();
  const modelInfo = getCurrentModelInfo(models, currentModel);
  const modelName = modelInfo
    ? getTranslatedModelName(modelInfo, t)
    : t("modelSelector.modelUnloaded");
  const modelStatus = loading
    ? t("modelSelector.loadingGeneric")
    : t("settings.sidebar.modelReady", { model: modelName });

  useEffect(() => {
    const fetchVersion = async () => {
      try {
        setVersion(await getVersion());
      } catch (error) {
        console.error("Failed to get app version:", error);
        setVersion("0.9.0");
      }
    };

    void fetchVersion();
  }, []);

  const availableSections = Object.entries(SECTIONS_CONFIG)
    .filter(([, config]) => config.enabled(settings))
    .map(([id, config]) => ({ id: id as SettingsSection, ...config }));

  return (
    <aside
      className="flex w-[218px] shrink-0 flex-col border-r border-black/[0.09] bg-sidebar bg-[linear-gradient(160deg,rgba(255,255,255,.35),rgba(255,255,255,0)_55%)] px-3 pb-3.5 pt-4 max-[700px]:w-[72px] dark:border-white/[0.10] dark:bg-[linear-gradient(160deg,rgba(255,255,255,.06),rgba(255,255,255,0)_55%),var(--color-sidebar)]"
      onMouseDown={startWindowDrag}
    >
      <div className="pt-[52px]">
        <div className="mb-2.5 flex items-center gap-2.5 px-1.5 pb-3 pt-4 max-[700px]:justify-center max-[700px]:px-0">
          <span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-[9px] bg-[linear-gradient(180deg,#3a3a3e,#151517)] text-white shadow-[0_1px_3px_rgba(0,0,0,.3)]">
            <BrandMark size={20} filled />
          </span>
          <span className="min-w-0 max-[700px]:hidden">
            {/* eslint-disable-next-line i18next/no-literal-string */}
            <span className="block text-[13.5px] font-semibold leading-[17px] tracking-[-0.01em]">
              OpenWispr
            </span>
            <span className="block text-[11px] leading-[14px] text-text-secondary">
              {t("settings.sidebar.version", { version })}
            </span>
          </span>
        </div>
      </div>

      <nav className="flex flex-col gap-px">
        {availableSections.map((section) => {
          const Icon = section.icon;
          const isActive = activeSection === section.id;

          return (
            <button
              key={section.id}
              type="button"
              onClick={() => onSectionChange(section.id)}
              aria-current={isActive ? "page" : undefined}
              aria-label={t(section.labelKey)}
              title={t(section.labelKey)}
              className={`flex min-h-[31px] items-center gap-[9px] rounded-[7px] px-2 py-[5px] text-[13px] leading-[18px] transition-colors max-[700px]:justify-center ${
                isActive
                  ? "bg-accent text-white"
                  : "text-text hover:bg-black/[0.055] dark:hover:bg-white/[0.07]"
              }`}
            >
              <span
                className={`flex h-[21px] w-[21px] shrink-0 items-center justify-center rounded-[5.5px] text-white ${
                  isActive
                    ? "bg-[linear-gradient(180deg,rgba(255,255,255,.34),rgba(255,255,255,.18))]"
                    : "bg-[linear-gradient(180deg,#9a9aa0,#7c7c83)] shadow-[0_.5px_1.5px_rgba(0,0,0,.25)]"
                }`}
              >
                <Icon width={13} height={13} className="shrink-0" />
              </span>
              <span className="truncate max-[700px]:hidden">
                {t(section.labelKey)}
              </span>
            </button>
          );
        })}
      </nav>

      <div className="mt-auto flex items-center gap-[7px] px-2 pt-2 text-[11.5px] leading-[15px] text-text-secondary max-[700px]:justify-center max-[700px]:px-0">
        <span className="h-[7px] w-[7px] shrink-0 rounded-full bg-ok" />
        <span className="truncate max-[700px]:hidden">{modelStatus}</span>
      </div>
    </aside>
  );
};

export default SettingsSidebar;
