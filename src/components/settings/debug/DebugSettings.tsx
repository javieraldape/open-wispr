import React from "react";
import { useTranslation } from "react-i18next";
import { WordCorrectionThreshold } from "./WordCorrectionThreshold";
import { LogLevelSelector } from "./LogLevelSelector";
import { LiveLogViewer } from "./LiveLogViewer";
import { PasteDelay } from "./PasteDelay";
import { RecordingBuffer } from "./RecordingBuffer";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { AlwaysOnMicrophone } from "../AlwaysOnMicrophone";
import { SoundPicker } from "../SoundPicker";
import { ClamshellMicrophoneSelector } from "../ClamshellMicrophoneSelector";
import { UpdateChecksToggle } from "../UpdateChecksToggle";
import { WhatsNewPreview } from "./WhatsNewPreview";

export const DebugSettings: React.FC = () => {
  const { t } = useTranslation();

  return (
    <div className="w-full space-y-6">
      <SettingsGroup title={t("settings.debug.title")}>
        <LogLevelSelector grouped={true} />
        <WhatsNewPreview descriptionMode="inline" grouped={true} />
        <UpdateChecksToggle descriptionMode="inline" grouped={true} />
        <SoundPicker
          label={t("settings.debug.soundTheme.label")}
          description={t("settings.debug.soundTheme.description")}
        />
        <WordCorrectionThreshold descriptionMode="inline" grouped={true} />
        <PasteDelay descriptionMode="inline" grouped={true} />
        <RecordingBuffer descriptionMode="inline" grouped={true} />
        <AlwaysOnMicrophone descriptionMode="inline" grouped={true} />
        <ClamshellMicrophoneSelector descriptionMode="inline" grouped={true} />
        <LiveLogViewer descriptionMode="inline" grouped={true} />
      </SettingsGroup>
    </div>
  );
};
