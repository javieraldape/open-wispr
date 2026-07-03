import React from "react";
import { useTranslation } from "react-i18next";
import { ShowOverlay } from "../ShowOverlay";
import { ModelUnloadTimeoutSetting } from "../ModelUnloadTimeout";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { StartHidden } from "../StartHidden";
import { AutostartToggle } from "../AutostartToggle";
import { ShowTrayIcon } from "../ShowTrayIcon";
import { PasteMethodSetting } from "../PasteMethod";
import { TypingToolSetting } from "../TypingTool";
import { ClipboardHandlingSetting } from "../ClipboardHandling";
import { AutoSubmit } from "../AutoSubmit";
import { PostProcessingToggle } from "../PostProcessingToggle";
import { AppendTrailingSpace } from "../AppendTrailingSpace";
import { HistoryLimit } from "../HistoryLimit";
import { RecordingRetentionPeriodSelector } from "../RecordingRetentionPeriod";
import { ExperimentalToggle } from "../ExperimentalToggle";
import { useSettings } from "../../../hooks/useSettings";
import { KeyboardImplementationSelector } from "../debug/KeyboardImplementationSelector";
import { VoiceActivityDetection } from "../VoiceActivityDetection";
import { AccelerationSelector } from "../AccelerationSelector";
import { LazyStreamClose } from "../LazyStreamClose";

export const AdvancedSettings: React.FC = () => {
  const { t } = useTranslation();
  const { getSetting } = useSettings();
  const experimentalEnabled = getSetting("experimental_enabled") || false;

  return (
    <div className="w-full space-y-6">
      <SettingsGroup title={t("settings.advanced.groups.app")}>
        <StartHidden descriptionMode="inline" grouped={true} />
        <AutostartToggle descriptionMode="inline" grouped={true} />
        <ShowTrayIcon descriptionMode="inline" grouped={true} />
        <ShowOverlay descriptionMode="inline" grouped={true} />
        <ModelUnloadTimeoutSetting descriptionMode="inline" grouped={true} />
        <ExperimentalToggle descriptionMode="inline" grouped={true} />
      </SettingsGroup>

      <SettingsGroup title={t("settings.advanced.groups.output")}>
        <PasteMethodSetting descriptionMode="inline" grouped={true} />
        <TypingToolSetting descriptionMode="inline" grouped={true} />
        <ClipboardHandlingSetting descriptionMode="inline" grouped={true} />
        <AutoSubmit descriptionMode="inline" grouped={true} />
      </SettingsGroup>

      <SettingsGroup title={t("settings.advanced.groups.transcription")}>
        <VoiceActivityDetection descriptionMode="inline" grouped={true} />
        <AppendTrailingSpace descriptionMode="inline" grouped={true} />
      </SettingsGroup>

      <SettingsGroup title={t("settings.advanced.groups.history")}>
        <HistoryLimit descriptionMode="inline" grouped={true} />
        <RecordingRetentionPeriodSelector
          descriptionMode="inline"
          grouped={true}
        />
      </SettingsGroup>

      {experimentalEnabled && (
        <SettingsGroup title={t("settings.advanced.groups.experimental")}>
          <PostProcessingToggle descriptionMode="inline" grouped={true} />
          <KeyboardImplementationSelector
            descriptionMode="inline"
            grouped={true}
          />
          <AccelerationSelector descriptionMode="inline" grouped={true} />
          <LazyStreamClose descriptionMode="inline" grouped={true} />
        </SettingsGroup>
      )}
    </div>
  );
};
