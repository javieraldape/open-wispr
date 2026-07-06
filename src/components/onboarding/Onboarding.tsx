import React, { useEffect, useMemo, useRef, useState } from "react";
import { Trans, useTranslation } from "react-i18next";
import { toast } from "sonner";
import { ChevronDown } from "lucide-react";
import type { ModelInfo } from "@/bindings";
import type { ModelCardStatus } from "./ModelCard";
import ModelCard from "./ModelCard";
import BrandLockup from "./BrandLockup";
import StepProgress from "./StepProgress";
import { useModelStore } from "../../stores/modelStore";
import { useSettingsStore } from "../../stores/settingsStore";
import { getLanguageLabel } from "../../lib/constants/languages";
import {
  getOnboardingModelGroups,
  isOnboardingLanguageIntent,
  ONBOARDING_LANGUAGE_OPTIONS,
  type OnboardingLanguageIntent,
} from "./modelSelection";

// Onboarding is 3 steps total (mic -> accessibility -> model, see App.tsx and
// the v4 design spec's "Surface 5 — First run"). This is step 3; steps 1-2
// are rendered by AccessibilityOnboarding.tsx.
const TOTAL_ONBOARDING_STEPS = 3;
const MODEL_STEP = 3;

interface OnboardingProps {
  onModelSelected: () => void;
}

const Onboarding: React.FC<OnboardingProps> = ({ onModelSelected }) => {
  const { t } = useTranslation();
  const {
    models,
    downloadModel,
    selectModel,
    downloadingModels,
    verifyingModels,
    extractingModels,
    downloadProgress,
    downloadStats,
  } = useModelStore();
  const [selectedModelId, setSelectedModelId] = useState<string | null>(null);
  const [showAll, setShowAll] = useState(false);
  const hasStartedSelection = useRef(false);
  const settings = useSettingsStore((state) => state.settings);
  const updateSetting = useSettingsStore((state) => state.updateSetting);
  const isSavingLanguage = useSettingsStore(
    (state) => state.isUpdating.selected_language || false,
  );
  const [languageIntent, setLanguageIntent] =
    useState<OnboardingLanguageIntent>("auto");

  const isBusy = selectedModelId !== null || isSavingLanguage;

  useEffect(() => {
    if (isOnboardingLanguageIntent(settings?.selected_language)) {
      setLanguageIntent(settings.selected_language);
    }
  }, [settings?.selected_language]);

  // Curate the download list: legacy (.bin/ONNX) downloads are deprecated and
  // never shown here (they still appear in the compatible section if already on
  // disk). The catalog ships 60+ models, so surfacing them all up front is
  // overwhelming. Instead we pre-select a language-aware default — the "hero" —
  // and hide the rest behind a collapsed disclosure.
  const { downloadable, heroModel, otherModels } = useMemo(() => {
    return getOnboardingModelGroups(models, languageIntent);
  }, [languageIntent, models]);

  // Watch for the selected model to finish downloading + verifying + extracting
  useEffect(() => {
    if (!selectedModelId) {
      hasStartedSelection.current = false;
      return;
    }

    const model = models.find((m) => m.id === selectedModelId);
    const stillDownloading = selectedModelId in downloadingModels;
    const stillVerifying = selectedModelId in verifyingModels;
    const stillExtracting = selectedModelId in extractingModels;

    if (
      model?.is_downloaded &&
      !stillDownloading &&
      !stillVerifying &&
      !stillExtracting &&
      !hasStartedSelection.current
    ) {
      hasStartedSelection.current = true;

      // Model is ready — select it and transition
      selectModel(selectedModelId).then((success) => {
        if (success) {
          onModelSelected();
        } else {
          toast.error(t("onboarding.errors.selectModel"));
          hasStartedSelection.current = false;
          setSelectedModelId(null);
        }
      });
    }
  }, [
    selectedModelId,
    models,
    downloadingModels,
    verifyingModels,
    extractingModels,
    selectModel,
    onModelSelected,
    t,
  ]);

  const handleDownloadModel = async (modelId: string) => {
    setSelectedModelId(modelId);

    // Error toast is handled centrally by the model-download-failed event listener
    // in modelStore — no toast here to avoid duplicates.
    const success = await downloadModel(modelId);
    if (!success) {
      setSelectedModelId(null);
    }
  };

  const handleSelectExistingModel = (modelId: string) => {
    setSelectedModelId(modelId);
  };

  const handleLanguageIntentChange = async (
    nextLanguageIntent: OnboardingLanguageIntent,
  ) => {
    setLanguageIntent(nextLanguageIntent);
    await updateSetting("selected_language", nextLanguageIntent);
  };

  const getLanguageOptionLabel = (option: OnboardingLanguageIntent): string => {
    if (option === "auto") {
      return t("settings.general.language.auto");
    }
    return getLanguageLabel(option) ?? option;
  };

  const getModelStatus = (modelId: string): ModelCardStatus => {
    if (modelId in extractingModels) return "extracting";
    if (modelId in verifyingModels) return "verifying";
    if (modelId in downloadingModels) return "downloading";
    return "downloadable";
  };

  const getExistingModelStatus = (modelId: string): ModelCardStatus => {
    if (selectedModelId === modelId) return "switching";
    return "available";
  };

  const getModelDownloadProgress = (modelId: string): number | undefined => {
    return downloadProgress[modelId]?.percentage;
  };

  const getModelDownloadSpeed = (modelId: string): number | undefined => {
    return downloadStats[modelId]?.speed;
  };

  return (
    <div className="h-screen w-screen flex flex-col p-6 gap-4 inset-0">
      <div className="max-w-md w-full mx-auto shrink-0 mb-2">
        <StepProgress current={MODEL_STEP} total={TOTAL_ONBOARDING_STEPS} />
      </div>
      <div className="flex flex-col items-center gap-2 shrink-0">
        <BrandLockup className="mb-1" />
        <h2 className="text-xl font-semibold text-text">
          {t("onboarding.modelStep.headline")}
        </h2>
        <p className="text-text/70 max-w-md mx-auto">
          <Trans
            i18nKey="onboarding.modelStep.body"
            components={{ b: <b /> }}
          />
        </p>
      </div>

      <div className="max-w-[600px] w-full mx-auto text-center flex-1 flex flex-col min-h-0">
        <div className="space-y-6 pb-6">
          <div className="space-y-2 text-left">
            <h2 className="text-sm font-medium text-text/60">
              {t("settings.general.language.title")}
            </h2>
            <div
              className="grid grid-cols-3 gap-1 rounded-lg border border-mid-gray/20 bg-content-bg p-1"
              role="radiogroup"
              aria-label={t("settings.general.language.title")}
            >
              {ONBOARDING_LANGUAGE_OPTIONS.map((option) => {
                const selected = languageIntent === option;
                return (
                  <button
                    key={option}
                    type="button"
                    role="radio"
                    aria-checked={selected}
                    disabled={selectedModelId !== null || isSavingLanguage}
                    onClick={() => handleLanguageIntentChange(option)}
                    className={`h-9 rounded-md px-3 text-sm font-medium transition-colors ${
                      selected
                        ? "bg-logo-primary text-white shadow-sm"
                        : "text-text/70 hover:bg-logo-primary/10 hover:text-text"
                    } ${
                      selectedModelId !== null || isSavingLanguage
                        ? "cursor-not-allowed opacity-60"
                        : ""
                    }`}
                  >
                    {getLanguageOptionLabel(option)}
                  </button>
                );
              })}
            </div>
          </div>

          {models.some((m: ModelInfo) => m.is_downloaded) && (
            <div className="space-y-3">
              <div className="text-left">
                <h2 className="text-sm font-medium text-text/60">
                  {t("onboarding.existingModelsTitle")}
                </h2>
              </div>
              {models
                .filter((m: ModelInfo) => m.is_downloaded)
                .map((model: ModelInfo) => (
                  <ModelCard
                    key={model.id}
                    model={model}
                    status={getExistingModelStatus(model.id)}
                    disabled={isBusy}
                    onSelect={handleSelectExistingModel}
                    showRecommended={false}
                  />
                ))}
            </div>
          )}

          {downloadable.length > 0 && (
            <div className="space-y-3">
              <div className="text-left">
                <h2 className="text-sm font-medium text-text/60">
                  {t("onboarding.downloadModelsTitle")}
                </h2>
              </div>

              {/* Pre-selected default: the recommended model, shown as a single
                  prominent hero so new users can proceed in one click. */}
              {heroModel && (
                <ModelCard
                  key={heroModel.id}
                  model={heroModel}
                  variant="featured"
                  status={getModelStatus(heroModel.id)}
                  disabled={isBusy}
                  onSelect={handleDownloadModel}
                  onDownload={handleDownloadModel}
                  downloadProgress={getModelDownloadProgress(heroModel.id)}
                  downloadSpeed={getModelDownloadSpeed(heroModel.id)}
                  showRecommended
                />
              )}

              {/* Everything else stays collapsed by default so the 60+ model
                  catalog doesn't overwhelm the first-run choice. */}
              {otherModels.length > 0 && (
                <button
                  type="button"
                  onClick={() => setShowAll((v) => !v)}
                  className="flex items-center justify-center gap-1.5 mx-auto py-1 text-sm font-medium text-text/60 hover:text-text transition-colors"
                >
                  {showAll
                    ? t("onboarding.showFewerModels")
                    : t("onboarding.showAllModels", {
                        total: downloadable.length,
                      })}
                  <ChevronDown
                    className={`w-4 h-4 transition-transform duration-200 ${
                      showAll ? "rotate-180" : ""
                    }`}
                  />
                </button>
              )}

              {showAll &&
                otherModels.map((model: ModelInfo) => (
                  <ModelCard
                    key={model.id}
                    model={model}
                    status={getModelStatus(model.id)}
                    disabled={isBusy}
                    onSelect={handleDownloadModel}
                    onDownload={handleDownloadModel}
                    downloadProgress={getModelDownloadProgress(model.id)}
                    downloadSpeed={getModelDownloadSpeed(model.id)}
                    showRecommended={false}
                  />
                ))}
            </div>
          )}
        </div>
      </div>
    </div>
  );
};

export default Onboarding;
