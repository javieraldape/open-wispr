import React from "react";
import {
  OPEN_WISPR_MARK_BARS,
  OPEN_WISPR_NAME,
} from "@/lib/brand/openWisprMark";

interface BrandMarkProps {
  width?: number | string;
  height?: number | string;
  size?: number | string;
  className?: string;
  strokeWidth?: number;
  /** Solid bars instead of hollow outlines — required for legibility below ~24px. */
  filled?: boolean;
}

export const BrandMark: React.FC<BrandMarkProps> = ({
  width,
  height,
  size = 22,
  className,
  strokeWidth = 3,
  filled = false,
}) => {
  const resolvedWidth = width ?? size;
  const resolvedHeight = height ?? size;
  const barWidth = filled ? 8 : 6;
  const barRadius = filled ? 4 : 3;

  return (
    <svg
      width={resolvedWidth}
      height={resolvedHeight}
      viewBox="0 0 64 64"
      fill={filled ? "currentColor" : "none"}
      stroke={filled ? "none" : "currentColor"}
      strokeWidth={filled ? 0 : strokeWidth}
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      {OPEN_WISPR_MARK_BARS.map((bar) => (
        <rect
          key={bar.x}
          x={bar.x - (barWidth - 6) / 2}
          y={bar.y}
          width={barWidth}
          height={bar.h}
          rx={barRadius}
        />
      ))}
    </svg>
  );
};

/** BrandMark with solid bars, matching lucide's IconProps shape for section nav. */
export const BrandMarkFilled: React.FC<Omit<BrandMarkProps, "filled">> = (
  props,
) => <BrandMark {...props} filled />;

interface BrandLockupProps {
  className?: string;
  markSize?: number;
  textClassName?: string;
}

const BrandLockup: React.FC<BrandLockupProps> = ({
  className,
  markSize = 22,
  textClassName = "text-[17px]",
}) => (
  <div
    className={`flex items-center gap-2 text-text ${className ?? ""}`}
    role="img"
    aria-label={OPEN_WISPR_NAME}
  >
    <BrandMark size={markSize} />
    {/* Product name, not user-facing copy. Keep literal, same convention as
        other brand/version strings. */}
    <span
      className={`${textClassName} font-bold`}
      style={{
        fontFamily:
          '"JetBrains Mono", ui-monospace, "SF Mono", Menlo, Consolas, monospace',
        letterSpacing: "-0.03em",
      }}
    >
      {OPEN_WISPR_NAME}
    </span>
  </div>
);

export default BrandLockup;
