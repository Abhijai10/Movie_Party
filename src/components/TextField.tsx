import type { InputHTMLAttributes } from "react";

type TextFieldProps = InputHTMLAttributes<HTMLInputElement> & {
  label: string;
  helperText?: string;
  error?: string | null;
};

export function TextField({ label, helperText, error, id, ...props }: TextFieldProps) {
  const inputId = id ?? label.toLowerCase().replaceAll(" ", "-");

  return (
    <label className="mp-field" htmlFor={inputId}>
      <span>{label}</span>
      <input id={inputId} aria-invalid={Boolean(error)} {...props} />
      {error ? (
        <small className="field-error" role="alert">
          {error}
        </small>
      ) : helperText ? (
        <small>{helperText}</small>
      ) : null}
    </label>
  );
}
