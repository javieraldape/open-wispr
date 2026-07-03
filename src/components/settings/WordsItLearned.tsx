import React, { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Plus, Trash2 } from "lucide-react";
import { toast } from "sonner";
import { commands, type StoredCorrectionDto } from "@/bindings";
import { Button } from "../ui/Button";
import { Input } from "../ui/Input";
import { SettingContainer } from "../ui/SettingContainer";

export const WordsItLearned: React.FC = React.memo(() => {
  const { t } = useTranslation();
  const [pairs, setPairs] = useState<StoredCorrectionDto[]>([]);
  const [heardText, setHeardText] = useState("");
  const [correctText, setCorrectText] = useState("");
  const [verbatim, setVerbatim] = useState(false);
  const [isLoading, setIsLoading] = useState(true);
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [deletingKeys, setDeletingKeys] = useState<Set<string>>(
    () => new Set(),
  );

  const loadPairs = useCallback(async () => {
    const result = await commands.correctionsList();
    if (result.status === "ok") {
      setPairs(result.data);
      return;
    }
    toast.error(t("settings.advanced.wordsItLearned.errors.load"));
  }, [t]);

  useEffect(() => {
    let cancelled = false;

    commands.correctionsList().then((result) => {
      if (cancelled) {
        return;
      }
      if (result.status === "ok") {
        setPairs(result.data);
      } else {
        toast.error(t("settings.advanced.wordsItLearned.errors.load"));
      }
      setIsLoading(false);
    });

    return () => {
      cancelled = true;
    };
  }, [t]);

  const canSubmit = useMemo(
    () => heardText.trim().length > 0 && correctText.trim().length > 0,
    [correctText, heardText],
  );

  const handleSubmit = async (event: React.FormEvent) => {
    event.preventDefault();
    if (!canSubmit || isSubmitting) {
      return;
    }

    setIsSubmitting(true);
    const result = await commands.correctionsAddManual(
      heardText.trim(),
      correctText.trim(),
      verbatim,
    );

    if (result.status === "ok") {
      setHeardText("");
      setCorrectText("");
      setVerbatim(false);
      await loadPairs();
    } else {
      toast.error(result.error);
    }
    setIsSubmitting(false);
  };

  const handleDelete = async (pair: StoredCorrectionDto) => {
    setDeletingKeys((current) => new Set(current).add(pair.heard_key));
    const result = await commands.correctionsDelete(pair.heard_key);

    if (result.status === "ok") {
      setPairs((current) =>
        current.filter((item) => item.heard_key !== pair.heard_key),
      );
    } else {
      toast.error(result.error);
    }

    setDeletingKeys((current) => {
      const next = new Set(current);
      next.delete(pair.heard_key);
      return next;
    });
  };

  return (
    <div className="divide-y divide-mid-gray/20">
      <SettingContainer
        title={t("settings.advanced.wordsItLearned.title")}
        description={t("settings.advanced.wordsItLearned.description")}
        descriptionMode="inline"
        grouped
        layout="stacked"
      >
        <form
          className="grid grid-cols-1 gap-2 sm:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto] sm:items-center"
          onSubmit={handleSubmit}
        >
          <Input
            type="text"
            value={heardText}
            onChange={(event) => setHeardText(event.target.value)}
            placeholder={t("settings.advanced.wordsItLearned.heardPlaceholder")}
            disabled={isSubmitting}
            aria-label={t("settings.advanced.wordsItLearned.heardLabel")}
          />
          <Input
            type="text"
            value={correctText}
            onChange={(event) => setCorrectText(event.target.value)}
            placeholder={t(
              "settings.advanced.wordsItLearned.correctPlaceholder",
            )}
            disabled={isSubmitting}
            aria-label={t("settings.advanced.wordsItLearned.correctLabel")}
          />
          <Button
            type="submit"
            variant="primary"
            size="md"
            disabled={!canSubmit || isSubmitting}
            className="inline-flex items-center justify-center gap-2 whitespace-nowrap"
          >
            <Plus className="h-4 w-4" aria-hidden="true" />
            <span>{t("settings.advanced.wordsItLearned.add")}</span>
          </Button>
          <label className="flex items-center gap-2 text-sm text-[#111111] sm:col-span-3">
            <input
              type="checkbox"
              checked={verbatim}
              onChange={(event) => setVerbatim(event.target.checked)}
              disabled={isSubmitting}
              className="h-4 w-4 accent-[#111111]"
            />
            <span>{t("settings.advanced.wordsItLearned.verbatimLabel")}</span>
          </label>
        </form>
      </SettingContainer>

      <div className="px-4 py-2">
        {isLoading ? (
          <p className="text-sm text-mid-gray">{t("common.loading")}</p>
        ) : pairs.length === 0 ? (
          <p className="text-sm text-mid-gray">
            {t("settings.advanced.wordsItLearned.empty")}
          </p>
        ) : (
          <ul className="space-y-1">
            {pairs.map((pair) => {
              const isDeleting = deletingKeys.has(pair.heard_key);
              return (
                <li
                  key={pair.heard_key}
                  className="flex min-h-9 items-center justify-between gap-3 rounded-md px-2 py-1"
                >
                  <div className="flex min-w-0 flex-wrap items-baseline gap-x-2 gap-y-1 text-sm">
                    <span
                      className="break-words font-medium line-through"
                      style={{ color: "#c2413b" }}
                    >
                      {pair.heard_text}
                    </span>
                    <span
                      className="break-words font-bold"
                      style={{ color: "#1e8e5a" }}
                    >
                      {pair.correct_text}
                    </span>
                  </div>
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    onClick={() => handleDelete(pair)}
                    disabled={isDeleting}
                    aria-label={t(
                      "settings.advanced.wordsItLearned.deletePair",
                      {
                        heard: pair.heard_text,
                        correct: pair.correct_text,
                      },
                    )}
                    title={t("common.delete")}
                    className="shrink-0 text-[#c2413b] hover:bg-[#c2413b]/10 hover:border-transparent focus:bg-[#c2413b]/10"
                  >
                    <Trash2 className="h-4 w-4" aria-hidden="true" />
                  </Button>
                </li>
              );
            })}
          </ul>
        )}
      </div>
    </div>
  );
});

WordsItLearned.displayName = "WordsItLearned";
