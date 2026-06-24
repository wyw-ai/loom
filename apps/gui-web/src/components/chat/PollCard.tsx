import type { ActionChoice } from "@/lib/types";

export function PollCard({
  choices,
  disabled,
  onChoose,
}: {
  choices: ActionChoice[];
  disabled: boolean;
  onChoose?: (choice: ActionChoice) => void;
}) {
  const totalVotes = choices.reduce((sum, choice) => sum + (choice.votes ?? 0), 0);
  const fallbackMax = choices.length;
  return (
    <div className="poll-card">
      {choices.map((choice, index) => {
        const votes = choice.votes ?? (totalVotes === 0 ? fallbackMax - index : 0);
        const denominator = totalVotes || fallbackMax || 1;
        const percent = Math.max(6, Math.round((votes / denominator) * 100));
        return (
          <button
            key={choice.id}
            type="button"
            className="poll-choice"
            disabled={disabled || !onChoose}
            onClick={() => onChoose?.(choice)}
          >
            <span className="poll-letter">{choice.id.slice(0, 1).toUpperCase()}</span>
            <span className="min-w-0 flex-1">
              <span className="block truncate text-sm font-semibold text-[#303849]">
                {choice.label}
              </span>
              <span className="mt-1 block h-0.5 overflow-hidden rounded-full bg-[#e7e9f3]">
                <span
                  className="block h-full rounded-full bg-[#5a47e9]"
                  style={{ width: `${percent}%` }}
                />
              </span>
            </span>
            <span className="w-8 text-right text-sm font-bold text-[#303849]">
              {votes}
            </span>
          </button>
        );
      })}
      <div className="mt-2 flex items-center gap-2 px-1 text-xs font-medium text-[#667085]">
        <span>{totalVotes || choices.length} votes</span>
        <span>•</span>
        <span>Poll closes soon</span>
      </div>
    </div>
  );
}
