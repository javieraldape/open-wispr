import React from "react";

interface SettingsGroupProps {
  title?: string;
  description?: string;
  children: React.ReactNode;
}

export const SettingsGroup: React.FC<SettingsGroupProps> = ({
  title,
  description,
  children,
}) => {
  return (
    <div>
      {title && (
        <div className="mx-0.5 mb-[7px] mt-[18px] first:mt-0">
          <h2 className="text-[12px] font-semibold leading-[16px] text-text-secondary">
            {title}
          </h2>
          {description && (
            <p className="mt-1 text-[11px] leading-[15px] text-text-secondary">
              {description}
            </p>
          )}
        </div>
      )}
      <div className="settings-card-ring overflow-visible rounded-[10px] bg-card">
        <div className="divide-y divide-separator">{children}</div>
      </div>
    </div>
  );
};
