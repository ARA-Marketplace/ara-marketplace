import { useState } from "react";

type Theme = "dark" | "light";

function applyTheme(theme: Theme) {
  const root = document.documentElement;
  if (theme === "dark") {
    root.classList.add("dark");
  } else {
    root.classList.remove("dark");
  }
  localStorage.setItem("ara-theme", theme);
}

export function useTheme() {
  const [theme, setTheme] = useState<Theme>(() => {
    const saved = localStorage.getItem("ara-theme") as Theme | null;
    const initial = saved ?? "dark";
    // Apply immediately so the DOM class is always in sync
    applyTheme(initial);
    return initial;
  });

  const toggle = () => {
    setTheme((current) => {
      const next: Theme = current === "dark" ? "light" : "dark";
      applyTheme(next);
      return next;
    });
  };

  return { theme, toggle };
}
