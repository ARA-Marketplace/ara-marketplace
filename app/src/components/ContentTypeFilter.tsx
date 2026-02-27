import type { ContentType } from "../lib/types";
import { CATEGORIES_BY_TYPE } from "../lib/categories";

const CONTENT_TYPES: Array<{ value: ContentType | "all"; label: string }> = [
  { value: "all", label: "All" },
  { value: "game", label: "Games" },
  { value: "music", label: "Music" },
  { value: "video", label: "Video" },
  { value: "document", label: "Docs" },
  { value: "software", label: "Software" },
  { value: "other", label: "Other" },
];

interface ContentTypeFilterProps {
  selectedType: ContentType | "all";
  selectedCategory: string | null;
  onTypeChange: (type: ContentType | "all") => void;
  onCategoryChange: (category: string | null) => void;
}

export default function ContentTypeFilter({
  selectedType,
  selectedCategory,
  onTypeChange,
  onCategoryChange,
}: ContentTypeFilterProps) {
  const categories =
    selectedType !== "all"
      ? CATEGORIES_BY_TYPE[selectedType] ?? []
      : [];

  return (
    <div className="space-y-2">
      {/* Type pills */}
      <div className="flex flex-wrap gap-2">
        {CONTENT_TYPES.map((t) => (
          <button
            key={t.value}
            onClick={() => {
              onTypeChange(t.value);
              onCategoryChange(null);
            }}
            className={`px-3 py-1.5 rounded-full text-xs font-medium transition-colors ${
              selectedType === t.value
                ? "bg-ara-600 text-white"
                : "bg-gray-100 dark:bg-gray-800 text-gray-600 dark:text-gray-300 hover:bg-gray-200 dark:hover:bg-gray-700"
            }`}
          >
            {t.label}
          </button>
        ))}
      </div>

      {/* Category pills */}
      {categories.length > 0 && (
        <div className="flex flex-wrap gap-1.5">
          <button
            onClick={() => onCategoryChange(null)}
            className={`px-2 py-1 rounded-full text-[11px] font-medium transition-colors ${
              !selectedCategory
                ? "bg-ara-100 dark:bg-ara-900/40 text-ara-700 dark:text-ara-300"
                : "bg-gray-50 dark:bg-gray-800/50 text-gray-500 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-gray-700"
            }`}
          >
            All
          </button>
          {categories.map((cat) => (
            <button
              key={cat}
              onClick={() => onCategoryChange(cat)}
              className={`px-2 py-1 rounded-full text-[11px] font-medium transition-colors ${
                selectedCategory === cat
                  ? "bg-ara-100 dark:bg-ara-900/40 text-ara-700 dark:text-ara-300"
                  : "bg-gray-50 dark:bg-gray-800/50 text-gray-500 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-gray-700"
              }`}
            >
              {cat}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
