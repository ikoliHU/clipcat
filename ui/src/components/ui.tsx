import type { ButtonHTMLAttributes, CSSProperties, InputHTMLAttributes, ReactNode, SelectHTMLAttributes } from "react";
import { cx } from "../lib/format";

// ---------- Gombok ----------

const BUTTON_VARIANTS = {
  default: "bg-panel-2 shadow-[inset_0_0_0_1px_var(--color-line)] font-medium enabled:hover:bg-panel-3",
  primary: "bg-accent text-accent-ink font-semibold enabled:hover:bg-accent-hover",
  danger: "bg-panel-2 shadow-[inset_0_0_0_1px_var(--color-line)] text-danger-soft font-medium enabled:hover:bg-panel-3",
  confirm: "bg-danger text-white font-medium",
  recording: "bg-danger text-white font-semibold tabular enabled:hover:bg-danger-hover",
};

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: keyof typeof BUTTON_VARIANTS;
  /** Full width, with content aligned to both edges (sidebar) */
  wide?: boolean;
}

export function Button({ variant = "default", wide, className, type = "button", ...props }: ButtonProps) {
  return (
    <button
      type={type}
      className={cx(
        "inline-flex h-[34px] items-center gap-2 whitespace-nowrap rounded-lg px-3.5 transition duration-150",
        "enabled:active:scale-[.97] disabled:opacity-40",
        wide ? "w-full justify-between" : "justify-center",
        BUTTON_VARIANTS[variant],
        className,
      )}
      {...props}
    />
  );
}

export const Kbd = ({ children }: { children: ReactNode }) => (
  <kbd className="ml-auto font-['Segoe_UI',sans-serif] text-[11.5px] font-medium leading-none tracking-[.02em] opacity-70">{children}</kbd>
);

export function LinkButton({ accent, className, ...props }: ButtonHTMLAttributes<HTMLButtonElement> & { accent?: boolean }) {
  return (
    <button
      type="button"
      className={cx(
        "flex items-center gap-2 border-t border-line px-3.5 py-2.5 text-left text-[13px] transition-colors duration-150",
        "enabled:hover:bg-white/3 enabled:active:bg-white/6 [&_svg]:size-4 [&_svg]:flex-none",
        accent ? "font-semibold text-accent tabular enabled:hover:text-accent-hover" : "text-muted hover:text-fg",
        className,
      )}
      {...props}
    />
  );
}

// ---------- Input fields ----------

const FIELD =
  "h-8 min-w-[220px] rounded-lg bg-panel-2 px-2.5 text-fg shadow-[inset_0_0_0_1px_var(--color-line)] transition duration-150 hover:bg-panel-3";

export function TextInput({ wide, className, ...props }: InputHTMLAttributes<HTMLInputElement> & { wide?: boolean }) {
  return (
    <input
      type="text"
      spellCheck={false}
      className={cx(FIELD, "select-text focus:shadow-[inset_0_0_0_1px_var(--color-accent),0_0_0_3px_var(--color-accent-soft)] focus:outline-none", wide && "min-w-[300px]", className)}
      {...props}
    />
  );
}

const CHEVRON = `url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 12 12' fill='none' stroke='%239d9da6' stroke-width='1.6' stroke-linecap='round' stroke-linejoin='round'%3E%3Cpath d='M3.5 5l2.5-2.5L8.5 5M3.5 7.5L6 10l2.5-2.5'/%3E%3C/svg%3E")`;

export function Select({ className, ...props }: SelectHTMLAttributes<HTMLSelectElement>) {
  return (
    <select
      className={cx(
        FIELD,
        "max-w-[340px] cursor-pointer appearance-none text-ellipsis bg-size-[12px] bg-position-[right_9px_center] bg-no-repeat pr-[30px]",
        "[&>option]:bg-panel-2 [&>option]:text-fg",
        "focus-visible:shadow-[inset_0_0_0_1px_var(--color-accent),0_0_0_3px_var(--color-accent-soft)] focus-visible:outline-none",
        className,
      )}
      style={{ backgroundImage: CHEVRON }}
      {...props}
    />
  );
}

// Slider: thin filled track, white thumb with a shadow
export function Range({ value, min, max, ...props }: Omit<InputHTMLAttributes<HTMLInputElement>, "value" | "min" | "max"> & { value: number; min: number; max: number }) {
  const fill = `${((value - min) / (max - min)) * 100}%`;
  return (
    <input
      type="range"
      value={value}
      min={min}
      max={max}
      style={{ "--fill": fill } as CSSProperties}
      className={cx(
        "m-0 h-5 w-[220px] cursor-pointer appearance-none bg-transparent focus-visible:outline-none",
        "[&::-webkit-slider-runnable-track]:h-1 [&::-webkit-slider-runnable-track]:rounded",
        "[&::-webkit-slider-runnable-track]:bg-[linear-gradient(to_right,var(--color-accent)_var(--fill),var(--color-panel-3)_var(--fill))]",
        "[&::-webkit-slider-thumb]:-mt-2 [&::-webkit-slider-thumb]:size-5 [&::-webkit-slider-thumb]:appearance-none [&::-webkit-slider-thumb]:rounded-full [&::-webkit-slider-thumb]:bg-white",
        "[&::-webkit-slider-thumb]:shadow-[0_1px_4px_rgba(0,0,0,.4),0_0_0_.5px_rgba(0,0,0,.2)] [&::-webkit-slider-thumb]:transition-transform [&::-webkit-slider-thumb]:duration-100",
        "active:[&::-webkit-slider-thumb]:scale-110",
        "focus-visible:[&::-webkit-slider-thumb]:shadow-[0_1px_4px_rgba(0,0,0,.4),0_0_0_4px_var(--color-focus)]",
      )}
      {...props}
    />
  );
}

export const Value = ({ children }: { children: ReactNode }) => (
  <span className="min-w-16 text-right font-semibold tabular">{children}</span>
);

interface SwitchProps extends Omit<InputHTMLAttributes<HTMLInputElement>, "type" | "size"> {
  small?: boolean;
}

export function Switch({ small, title, ...props }: SwitchProps) {
  return (
    <label className={cx("relative flex-none", small ? "h-[22px] w-9" : "h-[26px] w-11")} title={title}>
      <input type="checkbox" className="peer absolute size-0 opacity-0" {...props} />
      <span
        className={cx(
          "absolute inset-0 cursor-pointer rounded-full bg-panel-3 transition-colors duration-250 ease-spring peer-checked:bg-accent",
          "peer-disabled:cursor-default peer-disabled:opacity-50 peer-focus-visible:outline-2 peer-focus-visible:outline-offset-2 peer-focus-visible:outline-focus",
          "after:absolute after:top-0.5 after:left-0.5 after:rounded-full after:bg-white after:transition-transform after:duration-250 after:ease-spring",
          "after:shadow-[0_2px_4px_rgba(0,0,0,.3),0_0_0_.5px_rgba(0,0,0,.1)]",
          small ? "after:size-[18px] peer-checked:after:translate-x-[14px]" : "after:size-[22px] peer-checked:after:translate-x-[18px]",
        )}
      />
    </label>
  );
}

export function Keycap({ active, className, ...props }: ButtonHTMLAttributes<HTMLButtonElement> & { active?: boolean }) {
  return (
    <button
      type="button"
      className={cx(
        "h-8 min-w-[132px] rounded-lg bg-panel-2 px-3 text-center tracking-[.01em] tabular transition duration-150 hover:bg-panel-3 active:scale-[.97]",
        active
          ? "font-medium text-accent shadow-[inset_0_0_0_1px_var(--color-accent),0_0_0_3px_var(--color-accent-soft)]"
          : "font-semibold shadow-[inset_0_0_0_1px_var(--color-line),inset_0_-2px_0_rgba(0,0,0,.25)]",
        className,
      )}
      {...props}
    />
  );
}

// ---------- Settings list ----------

// Grouped list: section heading above the group, inset separators
export function Card({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div>
      <h2 className="mb-2 px-4 text-[12.5px] font-semibold tracking-[.01em] text-muted">{title}</h2>
      <div className="overflow-hidden rounded-xl bg-panel shadow-[inset_0_0_0_1px_var(--color-line)]">{children}</div>
    </div>
  );
}

export type HintTone = "error" | "warn";
export interface Hint {
  text: ReactNode;
  tone?: HintTone;
  className?: string;
}

export function Row({ label, hints = [], children }: { label: ReactNode; hints?: (Hint | string | null | false | undefined)[]; children: ReactNode }) {
  return (
    <div
      className={cx(
        "relative flex min-h-[52px] items-center gap-5 px-4 py-2.5",
        "[&+&]:before:absolute [&+&]:before:top-0 [&+&]:before:right-0 [&+&]:before:left-4 [&+&]:before:h-px [&+&]:before:bg-line",
      )}
    >
      <div className="min-w-0 flex-1">
        <span>{label}</span>
        {hints.map((hint, i) => {
          if (!hint) return null;
          const { text, tone, className } = typeof hint === "string" ? { text: hint } as Hint : hint;
          if (!text) return null;
          return (
            <div
              key={i}
              className={cx(
                "mt-px text-[12.5px] leading-[1.35]",
                tone === "error" ? "text-danger-soft" : tone === "warn" ? "text-warn-soft" : "text-muted",
                className,
              )}
            >
              {text}
            </div>
          );
        })}
      </div>
      <div className="flex items-center gap-2">{children}</div>
    </div>
  );
}
