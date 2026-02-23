import { ReactNode, useState } from "react";
import Navbar from "./Navbar";
import { useTheme } from "../hooks/useTheme";

interface LayoutProps {
  children: ReactNode;
}

function Layout({ children }: LayoutProps) {
  const { theme, toggle } = useTheme();

  const [collapsed, setCollapsed] = useState(() => {
    return localStorage.getItem("ara-nav-collapsed") === "true";
  });

  const toggleCollapsed = () => {
    setCollapsed((prev) => {
      const next = !prev;
      localStorage.setItem("ara-nav-collapsed", String(next));
      return next;
    });
  };

  return (
    <div className="flex h-screen overflow-hidden">
      <Navbar
        theme={theme}
        onToggleTheme={toggle}
        collapsed={collapsed}
        onToggleCollapsed={toggleCollapsed}
      />
      <main className="flex-1 overflow-y-auto bg-slate-50 dark:bg-slate-950">
        <div className="max-w-6xl mx-auto px-8 py-8">
          {children}
        </div>
      </main>
    </div>
  );
}

export default Layout;
