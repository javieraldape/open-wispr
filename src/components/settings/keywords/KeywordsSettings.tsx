import React from "react";
import { useTranslation } from "react-i18next";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { WordsItLearned } from "../WordsItLearned";

export const KeywordsSettings: React.FC = () => {
  const { t } = useTranslation();

  return (
    <div className="w-full space-y-6">
      <SettingsGroup title={t("settings.keywords.title")}>
        <WordsItLearned />
      </SettingsGroup>
    </div>
  );
};
