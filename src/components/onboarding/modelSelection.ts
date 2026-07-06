import type { ModelInfo } from "../../bindings";
import { supportsLanguageCode } from "../../lib/constants/languages";

export type OnboardingLanguageIntent = "auto" | "en" | "es";

const SPANISH_PARAKEET_TDT_V3_PREFIX =
  "handy-computer/parakeet-tdt-0.6b-v3-gguf/";

export const ONBOARDING_LANGUAGE_OPTIONS: OnboardingLanguageIntent[] = [
  "auto",
  "en",
  "es",
];

export const isOnboardingLanguageIntent = (
  value: string | undefined,
): value is OnboardingLanguageIntent =>
  value === "auto" || value === "en" || value === "es";

// Legacy = a blob (Url-sourced) .bin/ONNX model, kept runnable but no longer the
// advertised download (catalog GGUFs supersede it).
export const isLegacySource = (model: ModelInfo): boolean =>
  typeof model.source === "object" && "Url" in model.source;

const supportsOnboardingLanguage = (
  model: ModelInfo,
  languageIntent: OnboardingLanguageIntent,
): boolean =>
  languageIntent === "auto" ||
  supportsLanguageCode(model.supported_languages, languageIntent);

const isSpanishParakeetTdtV3Gguf = (model: ModelInfo): boolean =>
  model.engine_type === "TranscribeCpp" &&
  model.id.startsWith(SPANISH_PARAKEET_TDT_V3_PREFIX) &&
  supportsLanguageCode(model.supported_languages, "es");

const selectPreferredDownloadableModel = (
  downloadable: ModelInfo[],
  languageIntent: OnboardingLanguageIntent,
): ModelInfo | null => {
  if (languageIntent === "es") {
    return (
      downloadable.find(isSpanishParakeetTdtV3Gguf) ??
      downloadable.find((model) =>
        supportsOnboardingLanguage(model, languageIntent),
      ) ??
      null
    );
  }

  return (
    downloadable.find(
      (model) =>
        model.is_recommended &&
        supportsOnboardingLanguage(model, languageIntent),
    ) ??
    downloadable.find((model) =>
      supportsOnboardingLanguage(model, languageIntent),
    ) ??
    null
  );
};

export const getOnboardingModelGroups = (
  models: ModelInfo[],
  languageIntent: OnboardingLanguageIntent,
) => {
  const downloadable = models.filter(
    (model) => !model.is_downloaded && !isLegacySource(model),
  );
  const heroModel =
    selectPreferredDownloadableModel(downloadable, languageIntent) ??
    downloadable[0] ??
    null;
  const otherModels = heroModel
    ? downloadable.filter((model) => model.id !== heroModel.id)
    : downloadable;

  return { downloadable, heroModel, otherModels };
};
