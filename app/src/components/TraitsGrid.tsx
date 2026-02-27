interface Trait {
  label: string;
  value: string;
}

interface TraitsGridProps {
  traits: Trait[];
  className?: string;
}

export default function TraitsGrid({ traits, className }: TraitsGridProps) {
  if (traits.length === 0) return null;

  return (
    <div className={`grid grid-cols-2 sm:grid-cols-3 gap-3 ${className ?? ""}`}>
      {traits.map((trait, i) => (
        <div
          key={i}
          className="rounded-lg border border-ara-200 dark:border-ara-800 bg-ara-50/50 dark:bg-ara-900/20 p-3 text-center"
        >
          <div className="text-[10px] uppercase tracking-wider text-ara-500 dark:text-ara-400 font-medium mb-1">
            {trait.label}
          </div>
          <div className="text-sm font-semibold dark:text-white truncate">
            {trait.value}
          </div>
        </div>
      ))}
    </div>
  );
}
