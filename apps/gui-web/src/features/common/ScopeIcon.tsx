import clsx from "clsx";

export function ScopeIcon({
  kind,
  className,
}: {
  kind: "channel" | "thread";
  className?: string;
}) {
  if (kind === "thread") {
    return (
      <svg
        aria-hidden="true"
        viewBox="0 0 16 16"
        className={clsx("h-4 w-4 shrink-0", className)}
        fill="none"
      >
        <path
          d="M4.25 3.75h5.5a3 3 0 0 1 0 6H8.9"
          stroke="currentColor"
          strokeWidth="1.55"
          strokeLinecap="round"
          strokeLinejoin="round"
        />
        <path
          d="M6.25 7.25 3.75 9.75l2.5 2.5"
          stroke="currentColor"
          strokeWidth="1.55"
          strokeLinecap="round"
          strokeLinejoin="round"
        />
        <path
          d="M3.75 9.75h5.4"
          stroke="currentColor"
          strokeWidth="1.55"
          strokeLinecap="round"
        />
      </svg>
    );
  }

  return (
    <svg
      aria-hidden="true"
      viewBox="0 0 16 16"
      className={clsx("h-4 w-4 shrink-0", className)}
      fill="none"
    >
      <path
        d="M6.1 2.75 4.95 13.25M11.05 2.75 9.9 13.25"
        stroke="currentColor"
        strokeWidth="1.55"
        strokeLinecap="round"
      />
      <path
        d="M2.75 6.15h10.5M2.75 9.85h10.5"
        stroke="currentColor"
        strokeWidth="1.55"
        strokeLinecap="round"
      />
    </svg>
  );
}
