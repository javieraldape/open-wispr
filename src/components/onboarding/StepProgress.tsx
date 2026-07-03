import React from "react";
import { useTranslation } from "react-i18next";

/**
 * Thin ink progress indicator for the onboarding flow (v4 design spec: "3
 * steps, thin ink progress bars, one job per step"). Renders `total` equal
 * hairline segments; segments up to and including `current` are filled ink,
 * the rest stay at the faint hairline color.
 */
const StepProgress: React.FC<{ current: number; total: number }> = ({
  current,
  total,
}) => {
  const { t } = useTranslation();
  return (
    <div
      className="flex gap-1.5 w-full"
      role="progressbar"
      aria-valuemin={1}
      aria-valuemax={total}
      aria-valuenow={current}
      aria-label={t("onboarding.steps.progressLabel", {
        current,
        total,
      })}
    >
      {Array.from({ length: total }, (_, i) => (
        <span
          key={i}
          className={`h-[3px] flex-1 rounded-full transition-colors ${
            i < current ? "bg-text" : "bg-text/10"
          }`}
        />
      ))}
    </div>
  );
};

export default StepProgress;
