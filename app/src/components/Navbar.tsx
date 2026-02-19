import { Link, useLocation } from "react-router-dom";
import { useEffect } from "react";
import {
  useWeb3Modal,
  useWeb3ModalAccount,
} from "@web3modal/ethers/react";
import { useWalletStore } from "../store/walletStore";

function Navbar() {
  const location = useLocation();
  const { open } = useWeb3Modal();
  const { address: web3Address, isConnected } = useWeb3ModalAccount();
  const { address: storeAddress, onWalletConnected, onWalletDisconnected } =
    useWalletStore();

  // Sync Web3Modal connection state with wallet store + backend
  useEffect(() => {
    if (isConnected && web3Address && web3Address !== storeAddress) {
      onWalletConnected(web3Address);
    } else if (!isConnected && storeAddress) {
      onWalletDisconnected();
    }
  }, [isConnected, web3Address, storeAddress, onWalletConnected, onWalletDisconnected]);

  const navLinks = [
    { path: "/", label: "Marketplace" },
    { path: "/publish", label: "Publish" },
    { path: "/library", label: "Library" },
    { path: "/dashboard", label: "Dashboard" },
    { path: "/wallet", label: "Wallet" },
  ];

  return (
    <nav className="bg-white shadow-sm border-b border-gray-200">
      <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
        <div className="flex justify-between h-16">
          <div className="flex items-center space-x-8">
            <Link to="/" className="text-xl font-bold text-ara-700">
              Ara
            </Link>
            <div className="hidden sm:flex space-x-4">
              {navLinks.map((link) => (
                <Link
                  key={link.path}
                  to={link.path}
                  className={`px-3 py-2 rounded-md text-sm font-medium ${
                    location.pathname === link.path
                      ? "bg-ara-100 text-ara-700"
                      : "text-gray-500 hover:text-gray-700 hover:bg-gray-100"
                  }`}
                >
                  {link.label}
                </Link>
              ))}
            </div>
          </div>
          <div className="flex items-center">
            {storeAddress ? (
              <button
                onClick={() => open()}
                className="text-sm text-gray-600 font-mono hover:text-ara-700 transition-colors"
              >
                {storeAddress.slice(0, 6)}...{storeAddress.slice(-4)}
              </button>
            ) : (
              <button
                onClick={() => open()}
                className="bg-ara-600 text-white px-4 py-2 rounded-lg text-sm font-medium hover:bg-ara-700 transition-colors"
              >
                Connect Wallet
              </button>
            )}
          </div>
        </div>
      </div>
    </nav>
  );
}

export default Navbar;
