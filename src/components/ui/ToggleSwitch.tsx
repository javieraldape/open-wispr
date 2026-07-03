import React from "react";
import { SettingContainer } from "./SettingContainer";

interface ToggleSwitchProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
  isUpdating?: boolean;
  label: string;
  description: string;
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
  tooltipPosition?: "top" | "bottom";
}

export const ToggleSwitch: React.FC<ToggleSwitchProps> = ({
  checked,
  onChange,
  disabled = false,
  isUpdating = false,
  label,
  description,
  descriptionMode = "inline",
  grouped = false,
  tooltipPosition = "top",
}) => {
  return (
    <SettingContainer
      title={label}
      description={description}
      descriptionMode={descriptionMode}
      grouped={grouped}
      disabled={disabled}
      tooltipPosition={tooltipPosition}
    >
      <label
        className={
          disabled || isUpdating ? "cursor-not-allowed" : "cursor-pointer"
        }
      >
        <input
          type="checkbox"
          value=""
          className="sr-only peer"
          checked={checked}
          disabled={disabled || isUpdating}
          onChange={(e) => onChange(e.target.checked)}
        />
        <div className="peer relative h-[22px] w-[38px] rounded-full bg-[#d8d8dc] shadow-[inset_0_0_0_.5px_rgba(0,0,0,.06)] peer-checked:bg-accent peer-focus:outline-none peer-focus:ring-2 peer-focus:ring-accent/30 peer-disabled:opacity-40 dark:bg-[#5a5a5f] after:absolute after:start-px after:top-px after:h-[20px] after:w-[20px] after:rounded-full after:bg-white after:shadow-[0_1px_2.5px_rgba(0,0,0,.3),0_0_0_.5px_rgba(0,0,0,.05)] after:transition-transform after:content-[''] peer-checked:after:translate-x-[16px] rtl:peer-checked:after:-translate-x-[16px]" />
      </label>
      {isUpdating && (
        <div className="absolute inset-0 flex items-center justify-center">
          <div className="h-4 w-4 animate-spin rounded-full border-2 border-accent border-t-transparent"></div>
        </div>
      )}
    </SettingContainer>
  );
};
