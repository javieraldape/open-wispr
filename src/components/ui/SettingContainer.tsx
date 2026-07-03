import React, { useEffect, useRef, useState } from "react";
import { Tooltip } from "./Tooltip";

interface SettingContainerProps {
  title: string;
  description: string;
  children: React.ReactNode;
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
  layout?: "horizontal" | "stacked";
  disabled?: boolean;
  tooltipPosition?: "top" | "bottom";
}

export const SettingContainer: React.FC<SettingContainerProps> = ({
  title,
  description,
  children,
  descriptionMode = "inline",
  grouped = false,
  layout = "horizontal",
  disabled = false,
  tooltipPosition = "top",
}) => {
  const [showTooltip, setShowTooltip] = useState(false);
  const tooltipRef = useRef<HTMLDivElement>(null);

  // Handle click outside to close tooltip
  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      if (
        tooltipRef.current &&
        !tooltipRef.current.contains(event.target as Node)
      ) {
        setShowTooltip(false);
      }
    };

    if (showTooltip) {
      document.addEventListener("mousedown", handleClickOutside);
      return () =>
        document.removeEventListener("mousedown", handleClickOutside);
    }
  }, [showTooltip]);

  const toggleTooltip = () => {
    setShowTooltip(!showTooltip);
  };

  const containerClasses = grouped
    ? "px-[14px] py-[10px]"
    : "rounded-[10px] bg-card px-[14px] py-[10px] settings-card-ring";

  if (layout === "stacked") {
    if (descriptionMode === "tooltip") {
      return (
        <div className={containerClasses}>
          <div className="mb-2 flex items-center gap-2">
            <h3
              className={`text-[13px] font-normal leading-[17px] ${disabled ? "opacity-40" : ""}`}
            >
              {title}
            </h3>
            <div
              ref={tooltipRef}
              className="relative"
              onMouseEnter={() => setShowTooltip(true)}
              onMouseLeave={() => setShowTooltip(false)}
              onClick={toggleTooltip}
            >
              <svg
                className="h-4 w-4 cursor-help select-none text-text-secondary transition-colors duration-200 hover:text-accent"
                fill="none"
                stroke="currentColor"
                viewBox="0 0 24 24"
                aria-label="More information"
                role="button"
                tabIndex={0}
                onKeyDown={(e) => {
                  if (e.key === "Enter" || e.key === " ") {
                    e.preventDefault();
                    toggleTooltip();
                  }
                }}
              >
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  strokeWidth={2}
                  d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"
                />
              </svg>
              {showTooltip && (
                <Tooltip targetRef={tooltipRef} position="top">
                  <p className="text-center text-[12px] leading-relaxed">
                    {description}
                  </p>
                </Tooltip>
              )}
            </div>
          </div>
          <div className="w-full">{children}</div>
        </div>
      );
    }

    return (
      <div className={containerClasses}>
        <div className="mb-2">
          <h3
            className={`text-[13px] font-normal leading-[17px] ${disabled ? "opacity-40" : ""}`}
          >
            {title}
          </h3>
          <p
            className={`mt-px text-[11px] leading-[15px] text-text-secondary ${disabled ? "opacity-40" : ""}`}
          >
            {description}
          </p>
        </div>
        <div className="w-full">{children}</div>
      </div>
    );
  }

  // Horizontal layout (default)
  const horizontalContainerClasses = grouped
    ? "flex min-h-11 items-center justify-between gap-4 px-[14px] py-[10px]"
    : "settings-card-ring flex min-h-11 items-center justify-between gap-4 rounded-[10px] bg-card px-[14px] py-[10px]";

  if (descriptionMode === "tooltip") {
    return (
      <div className={horizontalContainerClasses}>
        <div className="max-w-[66%]">
          <div className="flex items-center gap-2">
            <h3
              className={`text-[13px] font-normal leading-[17px] ${disabled ? "opacity-40" : ""}`}
            >
              {title}
            </h3>
            <div
              ref={tooltipRef}
              className="relative"
              onMouseEnter={() => setShowTooltip(true)}
              onMouseLeave={() => setShowTooltip(false)}
              onClick={toggleTooltip}
            >
              <svg
                className="h-4 w-4 cursor-help select-none text-text-secondary transition-colors duration-200 hover:text-accent"
                fill="none"
                stroke="currentColor"
                viewBox="0 0 24 24"
                aria-label="More information"
                role="button"
                tabIndex={0}
                onKeyDown={(e) => {
                  if (e.key === "Enter" || e.key === " ") {
                    e.preventDefault();
                    toggleTooltip();
                  }
                }}
              >
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  strokeWidth={2}
                  d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"
                />
              </svg>
              {showTooltip && (
                <Tooltip targetRef={tooltipRef} position={tooltipPosition}>
                  <p className="text-center text-[12px] leading-relaxed">
                    {description}
                  </p>
                </Tooltip>
              )}
            </div>
          </div>
        </div>
        <div className="relative shrink-0">{children}</div>
      </div>
    );
  }

  return (
    <div className={horizontalContainerClasses}>
      <div className="max-w-[66%]">
        <h3
          className={`text-[13px] font-normal leading-[17px] ${disabled ? "opacity-40" : ""}`}
        >
          {title}
        </h3>
        <p
          className={`mt-px text-[11px] leading-[15px] text-text-secondary ${disabled ? "opacity-40" : ""}`}
        >
          {description}
        </p>
      </div>
      <div className="relative shrink-0">{children}</div>
    </div>
  );
};
