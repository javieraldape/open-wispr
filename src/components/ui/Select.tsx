import React from "react";
import SelectComponent from "react-select";
import CreatableSelect from "react-select/creatable";
import type {
  ActionMeta,
  Props as ReactSelectProps,
  SingleValue,
  StylesConfig,
} from "react-select";

export type SelectOption = {
  value: string;
  label: string;
  isDisabled?: boolean;
};

type BaseProps = {
  value: string | null;
  options: SelectOption[];
  placeholder?: string;
  disabled?: boolean;
  isLoading?: boolean;
  isClearable?: boolean;
  onChange: (value: string | null, action: ActionMeta<SelectOption>) => void;
  onBlur?: () => void;
  className?: string;
  formatCreateLabel?: (input: string) => string;
};

type CreatableProps = {
  isCreatable: true;
  onCreateOption: (value: string) => void;
};

type NonCreatableProps = {
  isCreatable?: false;
  onCreateOption?: never;
};

export type SelectProps = BaseProps & (CreatableProps | NonCreatableProps);

const hoverBackground =
  "color-mix(in srgb, var(--color-accent) 9%, transparent)";
const focusBackground =
  "color-mix(in srgb, var(--color-accent) 15%, transparent)";

const selectStyles: StylesConfig<SelectOption, false> = {
  control: (base, state) => ({
    ...base,
    minHeight: 26,
    borderRadius: 5.5,
    borderColor: "transparent",
    boxShadow: state.isFocused
      ? "0 0 0 .5px rgba(10,130,255,.65), 0 0 0 3px rgba(10,130,255,.15)"
      : "0 0 0 .5px rgba(0,0,0,.18), 0 .5px 2px rgba(0,0,0,.12)",
    backgroundColor: "var(--color-card)",
    fontSize: 13,
    color: "var(--color-text)",
    transition: "box-shadow 150ms ease, background-color 150ms ease",
    ":hover": {
      backgroundColor: "var(--color-card)",
    },
  }),
  valueContainer: (base) => ({
    ...base,
    paddingInline: 10,
    paddingBlock: 0,
  }),
  input: (base) => ({
    ...base,
    color: "var(--color-text)",
  }),
  singleValue: (base) => ({
    ...base,
    color: "var(--color-text)",
  }),
  dropdownIndicator: (base, state) => ({
    ...base,
    padding: 4,
    color: state.isFocused
      ? "var(--color-accent)"
      : "var(--color-text-secondary)",
    ":hover": {
      color: "var(--color-accent)",
    },
  }),
  clearIndicator: (base) => ({
    ...base,
    padding: 4,
    color: "var(--color-text-secondary)",
    ":hover": {
      color: "var(--color-accent)",
    },
  }),
  indicatorSeparator: () => ({ display: "none" }),
  menu: (provided) => ({
    ...provided,
    zIndex: 30,
    backgroundColor: "var(--color-card)",
    color: "var(--color-text)",
    border: ".5px solid color-mix(in srgb, var(--color-text) 14%, transparent)",
    boxShadow: "0 10px 30px rgba(15, 15, 15, 0.2)",
    fontSize: 13,
  }),
  option: (base, state) => ({
    ...base,
    backgroundColor: state.isSelected
      ? "var(--color-accent)"
      : state.isFocused
        ? hoverBackground
        : "transparent",
    color: state.isSelected ? "#fff" : "var(--color-text)",
    cursor: state.isDisabled ? "not-allowed" : base.cursor,
    opacity: state.isDisabled ? 0.5 : 1,
  }),
  placeholder: (base) => ({
    ...base,
    color: "var(--color-text-secondary)",
  }),
};

export const Select: React.FC<SelectProps> = React.memo(
  ({
    value,
    options,
    placeholder,
    disabled,
    isLoading,
    isClearable = true,
    onChange,
    onBlur,
    className = "",
    isCreatable,
    formatCreateLabel,
    onCreateOption,
  }) => {
    const selectValue = React.useMemo(() => {
      if (!value) return null;
      const existing = options.find((option) => option.value === value);
      if (existing) return existing;
      return { value, label: value, isDisabled: false };
    }, [value, options]);

    const handleChange = (
      option: SingleValue<SelectOption>,
      action: ActionMeta<SelectOption>,
    ) => {
      onChange(option?.value ?? null, action);
    };

    const sharedProps: Partial<ReactSelectProps<SelectOption, false>> = {
      className,
      classNamePrefix: "app-select",
      value: selectValue,
      options,
      onChange: handleChange,
      placeholder,
      isDisabled: disabled,
      isLoading,
      onBlur,
      isClearable,
      styles: selectStyles,
    };

    if (isCreatable) {
      return (
        <CreatableSelect<SelectOption, false>
          {...sharedProps}
          onCreateOption={onCreateOption}
          formatCreateLabel={formatCreateLabel}
        />
      );
    }

    return <SelectComponent<SelectOption, false> {...sharedProps} />;
  },
);

Select.displayName = "Select";
