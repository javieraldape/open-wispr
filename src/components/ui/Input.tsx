import React from "react";

interface InputProps extends React.InputHTMLAttributes<HTMLInputElement> {
  variant?: "default" | "compact";
}

export const Input: React.FC<InputProps> = ({
  className = "",
  variant = "default",
  disabled,
  ...props
}) => {
  const baseClasses =
    "text-start text-[13px] bg-card rounded-[5.5px] shadow-[0_0_0_.5px_rgba(0,0,0,.18),0_.5px_2px_rgba(0,0,0,.12)] transition-[box-shadow,background-color] duration-150";

  const interactiveClasses = disabled
    ? "opacity-40 cursor-not-allowed"
    : "focus:outline-none focus:shadow-[0_0_0_.5px_rgba(10,130,255,.65),0_0_0_3px_rgba(10,130,255,.15)]";

  const variantClasses = {
    default: "px-3 py-1.5",
    compact: "px-2 py-1",
  } as const;

  return (
    <input
      className={`${baseClasses} ${variantClasses[variant]} ${interactiveClasses} ${className}`}
      disabled={disabled}
      {...props}
    />
  );
};
