import React, { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ask } from "@tauri-apps/plugin-dialog";
import { ChevronDown, Globe, RefreshCw, Search } from "lucide-react";
import type { ModelCardStatus } from "@/components/onboarding";
import { ModelCard } from "@/components/onboarding";
import { useModelStore } from "@/stores/modelStore";
import {
  getLanguageLabel,
  MODEL_CAPABILITY_LANGUAGES,
  supportsLanguageCode,
} from "@/lib/constants/languages.ts";
import type { ModelInfo } from "@/bindings";

// check if model supports a language based on its supported_languages list
const modelSupportsLanguage = (model: ModelInfo, langCode: string): boolean => {
  return supportsLanguageCode(model.supported_languages, langCode);
};

// Legacy models are the blob (Url-sourced) .bin/ONNX downloads, superseded by
// the catalog GGUFs. They stay runnable when already on disk, but we no longer
// advertise the download.
const isLegacyModel = (model: ModelInfo): boolean =>
  typeof model.source === "object" && "Url" in model.source;

export const ModelsSettings: React.FC = () => {
  const { t } = useTranslation();
  const [switchingModelId, setSwitchingModelId] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState("");
  const [languageFilter, setLanguageFilter] = useState("all");
  const [languageDropdownOpen, setLanguageDropdownOpen] = useState(false);
  const [languageSearch, setLanguageSearch] = useState("");
  const languageDropdownRef = useRef<HTMLDivElement>(null);
  const languageSearchInputRef = useRef<HTMLInputElement>(null);
  const {
    models,
    currentModel,
    downloadingModels,
    downloadProgress,
    downloadStats,
    verifyingModels,
    extractingModels,
    loading,
    isRescanning,
    downloadModel,
    cancelDownload,
    selectModel,
    deleteModel,
    rescanLocalModels,
  } = useModelStore();

  // click outside handler for language dropdown
  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      if (
        languageDropdownRef.current &&
        !languageDropdownRef.current.contains(event.target as Node)
      ) {
        setLanguageDropdownOpen(false);
        setLanguageSearch("");
      }
    };
    document.addEventListener("mousedown", handleClickOutside);
    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, []);

  // focus search input when dropdown opens
  useEffect(() => {
    if (languageDropdownOpen && languageSearchInputRef.current) {
      languageSearchInputRef.current.focus();
    }
  }, [languageDropdownOpen]);

  // filtered languages for dropdown (exclude "auto")
  const filteredLanguages = useMemo(() => {
    return MODEL_CAPABILITY_LANGUAGES.filter((lang) =>
      lang.label.toLowerCase().includes(languageSearch.toLowerCase()),
    );
  }, [languageSearch]);

  // Get selected language label
  const selectedLanguageLabel = useMemo(() => {
    if (languageFilter === "all") {
      return t("settings.models.filters.allLanguages");
    }
    return getLanguageLabel(languageFilter) || "";
  }, [languageFilter, t]);

  const getModelStatus = (modelId: string): ModelCardStatus => {
    if (modelId in extractingModels) {
      return "extracting";
    }
    if (modelId in verifyingModels) {
      return "verifying";
    }
    if (modelId in downloadingModels) {
      return "downloading";
    }
    if (switchingModelId === modelId) {
      return "switching";
    }
    if (modelId === currentModel) {
      return "active";
    }
    const model = models.find((m: ModelInfo) => m.id === modelId);
    if (model?.is_downloaded) {
      return "available";
    }
    return "downloadable";
  };

  const getDownloadProgress = (modelId: string): number | undefined => {
    const progress = downloadProgress[modelId];
    return progress?.percentage;
  };

  const getDownloadSpeed = (modelId: string): number | undefined => {
    const stats = downloadStats[modelId];
    return stats?.speed;
  };

  const handleModelSelect = async (modelId: string) => {
    setSwitchingModelId(modelId);
    try {
      await selectModel(modelId);
    } finally {
      setSwitchingModelId(null);
    }
  };

  const handleModelDownload = async (modelId: string) => {
    await downloadModel(modelId);
  };

  const handleModelDelete = async (modelId: string) => {
    const model = models.find((m: ModelInfo) => m.id === modelId);
    const modelName = model?.name || modelId;
    const isActive = modelId === currentModel;

    const confirmed = await ask(
      isActive
        ? t("settings.models.deleteActiveConfirm", { modelName })
        : t("settings.models.deleteConfirm", { modelName }),
      {
        title: t("settings.models.deleteTitle"),
        kind: "warning",
      },
    );

    if (confirmed) {
      try {
        await deleteModel(modelId);
      } catch (err) {
        console.error(`Failed to delete model ${modelId}:`, err);
      }
    }
  };

  const handleModelCancel = async (modelId: string) => {
    try {
      await cancelDownload(modelId);
    } catch (err) {
      console.error(`Failed to cancel download for ${modelId}:`, err);
    }
  };

  // Filter models by search query (name + description) and language filter
  const filteredModels = useMemo(() => {
    const q = searchQuery.trim().toLowerCase();
    return models.filter((model: ModelInfo) => {
      // Hide deprecated legacy (.bin/ONNX) downloads unless already on disk.
      if (isLegacyModel(model) && !model.is_downloaded) return false;
      if (languageFilter !== "all") {
        if (!modelSupportsLanguage(model, languageFilter)) return false;
      }
      if (q) {
        const haystack = `${model.name} ${model.description}`.toLowerCase();
        if (!haystack.includes(q)) return false;
      }
      return true;
    });
  }, [models, languageFilter, searchQuery]);

  // Split filtered models into downloaded (including custom) and available sections
  const { downloadedModels, availableModels } = useMemo(() => {
    const downloaded: ModelInfo[] = [];
    const available: ModelInfo[] = [];

    for (const model of filteredModels) {
      if (
        model.is_custom ||
        model.is_downloaded ||
        model.id in downloadingModels ||
        model.id in extractingModels
      ) {
        downloaded.push(model);
      } else {
        available.push(model);
      }
    }

    // Sort: active model first, then non-custom, then custom at the bottom
    downloaded.sort((a, b) => {
      if (a.id === currentModel) return -1;
      if (b.id === currentModel) return 1;
      if (a.is_custom !== b.is_custom) return a.is_custom ? 1 : -1;
      return 0;
    });

    return {
      downloadedModels: downloaded,
      availableModels: available,
    };
  }, [filteredModels, downloadingModels, extractingModels, currentModel]);

  if (loading) {
    return (
      <div className="w-full">
        <div className="flex items-center justify-center py-16">
          <div className="h-8 w-8 animate-spin rounded-full border-2 border-accent border-t-transparent" />
        </div>
      </div>
    );
  }

  return (
    <div className="w-full space-y-4">
      <div className="mb-4">
        <h1 className="mb-1 text-[15px] font-semibold">
          {t("settings.models.title")}
        </h1>
        <p className="text-[12px] leading-[16px] text-text-secondary">
          {t("settings.models.description")}
        </p>
      </div>

      {/* Search bar — filter the catalog by name or description */}
      <div className="relative">
        <Search className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-text-secondary" />
        <input
          type="text"
          value={searchQuery}
          onChange={(e) => setSearchQuery(e.target.value)}
          placeholder={t("settings.models.searchPlaceholder")}
          className="w-full rounded-[5.5px] bg-card py-1.5 pl-9 pr-3 text-[13px] shadow-[0_0_0_.5px_rgba(0,0,0,.18),0_.5px_2px_rgba(0,0,0,.12)] placeholder:text-text-secondary focus:outline-none focus:shadow-[0_0_0_.5px_rgba(10,130,255,.65),0_0_0_3px_rgba(10,130,255,.15)]"
        />
      </div>

      {filteredModels.length > 0 ? (
        <div className="space-y-6">
          {/* Downloaded Models Section — header always visible so filter stays accessible */}
          <div className="space-y-3">
            <div className="flex items-center justify-between">
              <h2 className="text-[12px] font-semibold text-text-secondary">
                {t("settings.models.yourModels")}
              </h2>
              <div className="flex items-center gap-2">
                {/* Rescan local sources for models added outside Handy */}
                <button
                  type="button"
                  onClick={() => rescanLocalModels()}
                  disabled={isRescanning}
                  title={t("settings.models.rescan.tooltip")}
                  className="flex items-center gap-1.5 rounded-[5.5px] bg-card px-3 py-1.5 text-[13px] font-medium text-text-secondary shadow-[0_0_0_.5px_rgba(0,0,0,.18),0_.5px_2px_rgba(0,0,0,.12)] transition-colors hover:bg-black/[0.035] disabled:cursor-not-allowed disabled:opacity-40 dark:hover:bg-white/[0.06]"
                >
                  <RefreshCw
                    className={`w-3.5 h-3.5 ${isRescanning ? "animate-spin" : ""}`}
                  />
                  <span>{t("settings.models.rescan.label")}</span>
                </button>
                {/* Language filter dropdown */}
                <div className="relative" ref={languageDropdownRef}>
                  <button
                    type="button"
                    onClick={() =>
                      setLanguageDropdownOpen(!languageDropdownOpen)
                    }
                    className={`flex items-center gap-1.5 rounded-[5.5px] px-3 py-1.5 text-[13px] font-medium shadow-[0_0_0_.5px_rgba(0,0,0,.18),0_.5px_2px_rgba(0,0,0,.12)] transition-colors ${
                      languageFilter !== "all"
                        ? "bg-accent text-white"
                        : "bg-card text-text-secondary hover:bg-black/[0.035] dark:hover:bg-white/[0.06]"
                    }`}
                  >
                    <Globe className="w-3.5 h-3.5" />
                    <span className="max-w-[120px] truncate">
                      {selectedLanguageLabel}
                    </span>
                    <ChevronDown
                      className={`w-3.5 h-3.5 transition-transform ${
                        languageDropdownOpen ? "rotate-180" : ""
                      }`}
                    />
                  </button>

                  {languageDropdownOpen && (
                    <div className="absolute right-0 top-full z-50 mt-1 w-56 overflow-hidden rounded-[10px] bg-card shadow-lg settings-card-ring">
                      <div className="border-b border-separator p-2">
                        <input
                          ref={languageSearchInputRef}
                          type="text"
                          value={languageSearch}
                          onChange={(e) => setLanguageSearch(e.target.value)}
                          onKeyDown={(e) => {
                            if (
                              e.key === "Enter" &&
                              filteredLanguages.length > 0
                            ) {
                              setLanguageFilter(filteredLanguages[0].value);
                              setLanguageDropdownOpen(false);
                              setLanguageSearch("");
                            } else if (e.key === "Escape") {
                              setLanguageDropdownOpen(false);
                              setLanguageSearch("");
                            }
                          }}
                          placeholder={t(
                            "settings.general.language.searchPlaceholder",
                          )}
                          className="w-full rounded-[5.5px] bg-card px-2 py-1 text-[13px] shadow-[0_0_0_.5px_rgba(0,0,0,.18),0_.5px_2px_rgba(0,0,0,.12)] focus:outline-none focus:shadow-[0_0_0_.5px_rgba(10,130,255,.65),0_0_0_3px_rgba(10,130,255,.15)]"
                        />
                      </div>
                      <div className="max-h-48 overflow-y-auto">
                        <button
                          type="button"
                          onClick={() => {
                            setLanguageFilter("all");
                            setLanguageDropdownOpen(false);
                            setLanguageSearch("");
                          }}
                          className={`w-full px-3 py-1.5 text-left text-[13px] transition-colors ${
                            languageFilter === "all"
                              ? "bg-accent text-white"
                              : "hover:bg-black/[0.055] dark:hover:bg-white/[0.07]"
                          }`}
                        >
                          {t("settings.models.filters.allLanguages")}
                        </button>
                        {filteredLanguages.map((lang) => (
                          <button
                            key={lang.value}
                            type="button"
                            onClick={() => {
                              setLanguageFilter(lang.value);
                              setLanguageDropdownOpen(false);
                              setLanguageSearch("");
                            }}
                            className={`w-full px-3 py-1.5 text-left text-[13px] transition-colors ${
                              languageFilter === lang.value
                                ? "bg-accent text-white"
                                : "hover:bg-black/[0.055] dark:hover:bg-white/[0.07]"
                            }`}
                          >
                            {lang.label}
                          </button>
                        ))}
                        {filteredLanguages.length === 0 && (
                          <div className="px-3 py-2 text-center text-[13px] text-text-secondary">
                            {t("settings.general.language.noResults")}
                          </div>
                        )}
                      </div>
                    </div>
                  )}
                </div>
              </div>
            </div>
            {downloadedModels.map((model: ModelInfo) => (
              <ModelCard
                key={model.id}
                model={model}
                status={getModelStatus(model.id)}
                onSelect={handleModelSelect}
                onDownload={handleModelDownload}
                onDelete={handleModelDelete}
                onCancel={handleModelCancel}
                downloadProgress={getDownloadProgress(model.id)}
                downloadSpeed={getDownloadSpeed(model.id)}
                showRecommended={false}
              />
            ))}
          </div>

          {/* Available Models Section */}
          {availableModels.length > 0 && (
            <div className="space-y-3">
              <h2 className="text-[12px] font-semibold text-text-secondary">
                {t("settings.models.availableModels")}
              </h2>
              {availableModels.map((model: ModelInfo) => (
                <ModelCard
                  key={model.id}
                  model={model}
                  status={getModelStatus(model.id)}
                  onSelect={handleModelSelect}
                  onDownload={handleModelDownload}
                  onDelete={handleModelDelete}
                  onCancel={handleModelCancel}
                  downloadProgress={getDownloadProgress(model.id)}
                  downloadSpeed={getDownloadSpeed(model.id)}
                  showRecommended={true}
                />
              ))}
            </div>
          )}
        </div>
      ) : (
        <div className="py-8 text-center text-text-secondary">
          {t("settings.models.noModelsMatch")}
        </div>
      )}
    </div>
  );
};
