import { expect, test } from "@playwright/test";
import type { ModelInfo } from "../src/bindings";
import {
  getOnboardingModelGroups,
  isLegacySource,
} from "../src/components/onboarding/modelSelection";

const model = (overrides: Partial<ModelInfo>): ModelInfo => ({
  id: "base",
  name: "Base",
  description: "",
  filename: "base.gguf",
  source: {
    HuggingFace: {
      repo_id: "handy-computer/base-gguf",
      revision: "0123456789012345678901234567890123456789",
      sha256: "0".repeat(64),
    },
  },
  size_mb: 1,
  is_downloaded: false,
  is_downloading: false,
  partial_size: 0,
  is_directory: false,
  engine_type: "TranscribeCpp",
  accuracy_score: 0,
  speed_score: 0,
  supports_translation: false,
  is_recommended: false,
  supported_languages: ["en"],
  supports_language_selection: false,
  is_custom: false,
  supports_streaming: false,
  supports_language_detection: false,
  ...overrides,
});

test.describe("onboarding model selection", () => {
  test("keeps legacy URL models out of first-run downloads", () => {
    expect(
      isLegacySource(
        model({
          source: {
            Url: { url: "https://example.com/model.tar.gz", sha256: null },
          },
        }),
      ),
    ).toBe(true);
  });

  test("uses the fastest recommended English model by default", () => {
    const english = model({
      id: "handy-computer/parakeet-unified-en-0.6b-gguf/parakeet-unified-en-0.6b-Q8_0.gguf",
      is_recommended: true,
      supported_languages: ["en"],
    });
    const spanish = model({
      id: "handy-computer/parakeet-tdt-0.6b-v3-gguf/parakeet-tdt-0.6b-v3-Q8_0.gguf",
      is_recommended: true,
      supported_languages: ["en", "es"],
      supports_language_selection: true,
      supports_language_detection: true,
    });

    const { heroModel } = getOnboardingModelGroups([english, spanish], "auto");

    expect(heroModel?.id).toBe(english.id);
  });

  test("selects GGUF Parakeet TDT v3 when Spanish is chosen", () => {
    const english = model({
      id: "handy-computer/parakeet-unified-en-0.6b-gguf/parakeet-unified-en-0.6b-Q8_0.gguf",
      is_recommended: true,
      supported_languages: ["en"],
    });
    const legacyParakeet = model({
      id: "parakeet-tdt-0.6b-v3",
      filename: "parakeet-tdt-0.6b-v3-int8",
      source: {
        Url: {
          url: "https://blob.handy.computer/parakeet-v3-int8.tar.gz",
          sha256: null,
        },
      },
      engine_type: "Parakeet",
      supported_languages: ["en"],
    });
    const spanishParakeet = model({
      id: "handy-computer/parakeet-tdt-0.6b-v3-gguf/parakeet-tdt-0.6b-v3-Q8_0.gguf",
      is_recommended: true,
      supported_languages: ["en", "es"],
      supports_language_selection: true,
      supports_language_detection: true,
    });

    const { downloadable, heroModel } = getOnboardingModelGroups(
      [english, legacyParakeet, spanishParakeet],
      "es",
    );

    expect(downloadable.map((m) => m.id)).not.toContain(legacyParakeet.id);
    expect(heroModel?.id).toBe(spanishParakeet.id);
    expect(heroModel?.engine_type).toBe("TranscribeCpp");
  });
});
