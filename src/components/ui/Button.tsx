import React from "react";

interface ButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  variant?:
    | "primary"
    | "primary-soft"
    | "secondary"
    | "danger"
    | "danger-ghost"
    | "ghost";
  size?: "sm" | "md" | "lg";
}

export const Button: React.FC<ButtonProps> = ({
  children,
  className = "",
  variant = "primary",
  size = "md",
  ...props
}) => {
  const baseClasses =
    "font-medium rounded-[5.5px] focus:outline-none transition-colors disabled:opacity-40 disabled:cursor-not-allowed cursor-pointer";

  const variantClasses = {
    primary:
      "text-white bg-accent shadow-[0_0_0_.5px_rgba(0,0,0,.12)] hover:bg-accent/90 focus:ring-2 focus:ring-accent/25",
    "primary-soft":
      "text-accent bg-accent/12 hover:bg-accent/18 focus:ring-2 focus:ring-accent/20",
    secondary:
      "bg-card text-text shadow-[0_0_0_.5px_rgba(0,0,0,.18),0_.5px_2px_rgba(0,0,0,.12)] hover:bg-black/[0.035] focus:ring-2 focus:ring-accent/20 dark:hover:bg-white/[0.06]",
    danger:
      "text-white bg-red-600 shadow-[0_0_0_.5px_rgba(0,0,0,.12)] hover:bg-red-700 focus:ring-2 focus:ring-red-500/25",
    "danger-ghost": "text-red-500 hover:bg-red-500/10 focus:bg-red-500/20",
    ghost:
      "text-current hover:bg-black/[0.055] focus:bg-black/[0.08] dark:hover:bg-white/[0.07] dark:focus:bg-white/[0.10]",
  };

  const sizeClasses = {
    sm: "px-2.5 py-1 text-[12px]",
    md: "px-3 py-[3px] text-[13px]",
    lg: "px-4 py-1.5 text-[14px]",
  };

  return (
    <button
      className={`${baseClasses} ${variantClasses[variant]} ${sizeClasses[size]} ${className}`}
      {...props}
    >
      {children}
    </button>
  );
};
