import React from "react";
import { SettingContainer } from "./SettingContainer";

interface SliderProps {
  value: number;
  onChange: (value: number) => void;
  min: number;
  max: number;
  step?: number;
  disabled?: boolean;
  label: string;
  description: string;
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
  showValue?: boolean;
  formatValue?: (value: number) => string;
}

export const Slider: React.FC<SliderProps> = ({
  value,
  onChange,
  min,
  max,
  step = 0.01,
  disabled = false,
  label,
  description,
  descriptionMode = "inline",
  grouped = false,
  showValue = true,
  formatValue = (v) => v.toFixed(2),
}) => {
  const handleChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    onChange(parseFloat(e.target.value));
  };

  return (
    <SettingContainer
      title={label}
      description={description}
      descriptionMode={descriptionMode}
      grouped={grouped}
      layout="horizontal"
      disabled={disabled}
    >
      <div className="w-[210px] max-w-[35vw]">
        <div className="flex h-6 items-center gap-2">
          <input
            type="range"
            min={min}
            max={max}
            step={step}
            value={value}
            onChange={handleChange}
            disabled={disabled}
            className="settings-slider h-1 flex-grow cursor-pointer appearance-none rounded-full focus:outline-none focus:ring-2 focus:ring-accent/25 disabled:cursor-not-allowed disabled:opacity-40"
            style={{
              background: `linear-gradient(to right, var(--color-accent) ${
                ((value - min) / (max - min)) * 100
              }%, #d8d8dc ${((value - min) / (max - min)) * 100}%)`,
            }}
          />
          {showValue && (
            <span className="w-12 text-end text-[12px] font-medium text-text-secondary">
              {formatValue(value)}
            </span>
          )}
        </div>
      </div>
    </SettingContainer>
  );
};
