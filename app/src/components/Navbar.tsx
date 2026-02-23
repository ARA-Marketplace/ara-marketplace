import { Link, useLocation } from "react-router-dom";
import { useEffect } from "react";
import { useWeb3Modal, useWeb3ModalAccount } from "@web3modal/ethers/react";
import { useWalletStore } from "../store/walletStore";
import {
  IconStore, IconUpload, IconLibrary, IconChart, IconWallet,
  IconSun, IconMoon, IconChevronLeft, IconChevronRight,
} from "./Icons";
import AraLogo from "../assets/AraLogo";

interface NavbarProps {
  theme: "dark" | "light";
  onToggleTheme: () => void;
  collapsed: boolean;
  onToggleCollapsed: () => void;
}

const NAV_LINKS = [
  { path: "/",          label: "Marketplace", Icon: IconStore   },
  { path: "/publish",   label: "Publish",     Icon: IconUpload  },
  { path: "/library",   label: "Library",     Icon: IconLibrary },
  { path: "/dashboard", label: "Dashboard",   Icon: IconChart   },
  { path: "/wallet",    label: "Wallet",      Icon: IconWallet  },
];

function Navbar({ theme, onToggleTheme, collapsed, onToggleCollapsed }: NavbarProps) {
  const location = useLocation();
  const { open } = useWeb3Modal();
  const { address: web3Address, isConnected } = useWeb3ModalAccount();
  const { address: storeAddress, onWalletConnected, onWalletDisconnected } = useWalletStore();

  useEffect(() => {
    if (isConnected && web3Address && web3Address !== storeAddress) {
      onWalletConnected(web3Address);
    } else if (!isConnected && storeAddress) {
      onWalletDisconnected();
    }
  }, [isConnected, web3Address, storeAddress, onWalletConnected, onWalletDisconnected]);

  return (
    <aside
      className={`flex flex-col flex-shrink-0 h-full overflow-hidden
                  bg-white dark:bg-slate-950
                  border-r border-slate-200 dark:border-slate-800
                  transition-[width] duration-200 ease-in-out
                  ${collapsed ? "w-[56px]" : "w-52"}`}
    >
      {/* Logo */}
      <div
        className={`flex items-center h-[57px] flex-shrink-0 border-b border-slate-200 dark:border-slate-800
                    ${collapsed ? "justify-center" : "gap-3 px-4"}`}
      >
        <div className="w-8 h-8 rounded-lg bg-ara-600 flex items-center justify-center flex-shrink-0">
          <AraLogo className="w-[18px] h-[18px] text-white" />
        </div>
        {!collapsed && (
          <div className="overflow-hidden">
            <p className="font-bold text-slate-900 dark:text-slate-100 text-sm leading-none whitespace-nowrap">Ara</p>
            <p className="text-[10px] text-slate-400 dark:text-slate-600 mt-0.5 leading-none whitespace-nowrap">Marketplace</p>
          </div>
        )}
      </div>

      {/* Nav links */}
      <nav className="flex-1 px-2 py-3 space-y-0.5 overflow-y-auto overflow-x-hidden">
        {NAV_LINKS.map(({ path, label, Icon }) => {
          const active = location.pathname === path;
          return (
            <Link
              key={path}
              to={path}
              title={collapsed ? label : undefined}
              className={`flex items-center rounded-lg text-sm font-medium transition-colors
                          ${collapsed ? "justify-center p-2.5" : "gap-3 px-3 py-2.5"}
                          ${active
                            ? "bg-ara-50 dark:bg-ara-950/60 text-ara-700 dark:text-ara-400"
                            : "text-slate-600 dark:text-slate-400 hover:bg-slate-100 dark:hover:bg-slate-900 hover:text-slate-900 dark:hover:text-slate-200"
                          }`}
            >
              <Icon
                className={`w-[18px] h-[18px] flex-shrink-0 ${
                  active ? "text-ara-600 dark:text-ara-400" : ""
                }`}
              />
              {!collapsed && <span className="truncate">{label}</span>}
            </Link>
          );
        })}
      </nav>

      {/* Bottom controls */}
      <div className="px-2 pb-2 pt-2 space-y-0.5 border-t border-slate-200 dark:border-slate-800 flex-shrink-0">
        {/* Theme toggle */}
        <button
          onClick={onToggleTheme}
          title={collapsed ? (theme === "dark" ? "Switch to light mode" : "Switch to dark mode") : undefined}
          className={`flex items-center w-full rounded-lg text-sm font-medium
                       text-slate-600 dark:text-slate-400 hover:bg-slate-100 dark:hover:bg-slate-900
                       hover:text-slate-900 dark:hover:text-slate-200 transition-colors
                       ${collapsed ? "justify-center p-2.5" : "gap-3 px-3 py-2.5"}`}
        >
          {theme === "dark"
            ? <IconSun className="w-[18px] h-[18px] flex-shrink-0" />
            : <IconMoon className="w-[18px] h-[18px] flex-shrink-0" />}
          {!collapsed && (
            <span className="truncate">{theme === "dark" ? "Light mode" : "Dark mode"}</span>
          )}
        </button>

        {/* Wallet */}
        {storeAddress ? (
          <button
            onClick={() => open()}
            title={collapsed ? `${storeAddress.slice(0, 6)}…${storeAddress.slice(-4)}` : undefined}
            className={`flex items-center w-full rounded-lg transition-colors
                         text-slate-600 dark:text-slate-400 hover:bg-slate-100 dark:hover:bg-slate-900
                         hover:text-slate-900 dark:hover:text-slate-200
                         ${collapsed ? "justify-center p-2.5" : "gap-3 px-3 py-2.5"}`}
          >
            <div className="w-[18px] h-[18px] rounded-full bg-emerald-500 flex-shrink-0 ring-2 ring-emerald-200 dark:ring-emerald-900/60" />
            {!collapsed && (
              <span className="font-mono text-xs truncate">
                {storeAddress.slice(0, 6)}…{storeAddress.slice(-4)}
              </span>
            )}
          </button>
        ) : (
          <button
            onClick={() => open()}
            title={collapsed ? "Connect Wallet" : undefined}
            className={`flex items-center w-full rounded-lg text-sm font-medium
                         bg-ara-600 hover:bg-ara-500 text-white transition-colors
                         ${collapsed ? "justify-center p-2.5" : "gap-3 px-3 py-2.5"}`}
          >
            <IconWallet className="w-[18px] h-[18px] flex-shrink-0" />
            {!collapsed && <span className="truncate">Connect Wallet</span>}
          </button>
        )}

        {/* Collapse toggle */}
        <button
          onClick={onToggleCollapsed}
          title={collapsed ? "Expand sidebar" : "Collapse sidebar"}
          className={`flex items-center w-full rounded-lg text-xs
                       text-slate-400 dark:text-slate-600
                       hover:bg-slate-100 dark:hover:bg-slate-800
                       hover:text-slate-600 dark:hover:text-slate-400
                       transition-colors
                       ${collapsed ? "justify-center p-2.5" : "gap-2 px-3 py-2"}`}
        >
          {collapsed
            ? <IconChevronRight className="w-4 h-4 flex-shrink-0" />
            : <>
                <IconChevronLeft className="w-4 h-4 flex-shrink-0" />
                <span>Collapse</span>
              </>
          }
        </button>
      </div>
    </aside>
  );
}

export default Navbar;
