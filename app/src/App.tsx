import { useEffect } from "react";
import { Routes, Route, useNavigate } from "react-router-dom";
import { listen } from "@tauri-apps/api/event";
import { check } from "@tauri-apps/plugin-updater";
import { ask } from "@tauri-apps/plugin-dialog";
import Layout from "./components/Layout";
import Marketplace from "./pages/Marketplace";
import ContentDetail from "./pages/ContentDetail";
import Publish from "./pages/Publish";
import Library from "./pages/Library";
import Dashboard from "./pages/Dashboard";
import Wallet from "./pages/Wallet";
import CollectionDetailPage from "./pages/CollectionDetail";
import Collections from "./pages/Collections";

// Apply saved theme before first render to avoid flash
const savedTheme = localStorage.getItem("ara-theme") ?? "dark";
if (savedTheme === "dark") {
  document.documentElement.classList.add("dark");
} else {
  document.documentElement.classList.remove("dark");
}

function App() {
  const navigate = useNavigate();

  // Check for app updates on startup (release builds only)
  useEffect(() => {
    const checkForUpdates = async () => {
      try {
        const update = await check();
        if (!update?.available) return;
        const yes = await ask(
          `Version ${update.version} is available.\n\n${update.body ?? ""}\n\nInstall now?`,
          { title: "Update Available", kind: "info" }
        );
        if (yes) {
          await update.downloadAndInstall();
        }
      } catch {
        // Silently ignore — update check is best-effort
      }
    };
    // Small delay so the window is fully rendered before showing a dialog
    const t = setTimeout(checkForUpdates, 3000);
    return () => clearTimeout(t);
  }, []);

  // Listen for deep link navigation events (ara:// protocol)
  useEffect(() => {
    const unlisten = listen<string>("deep-link-navigate", (event) => {
      navigate(event.payload);
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [navigate]);

  return (
    <Layout>
      <Routes>
        <Route path="/" element={<Marketplace />} />
        <Route path="/content/:contentId" element={<ContentDetail />} />
        <Route path="/collections" element={<Collections />} />
        <Route path="/collections/:collectionId" element={<CollectionDetailPage />} />
        <Route path="/publish" element={<Publish />} />
        <Route path="/library" element={<Library />} />
        <Route path="/dashboard" element={<Dashboard />} />
        <Route path="/wallet" element={<Wallet />} />
      </Routes>
    </Layout>
  );
}

export default App;
