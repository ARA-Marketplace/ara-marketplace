import { useState, useEffect, useRef } from "react";
import { useNavigate } from "react-router-dom";
import { open } from "@tauri-apps/plugin-dialog";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import {
  publishContent,
  confirmPublish,
  getMyCollections,
  createCollection,
  confirmCreateCollection,
  addToCollection,
  confirmAddToCollection,
  getSupportedTokens,
  prepareArweaveUpload,
  executeArweaveUpload,
} from "../lib/tauri";
import type { CollectionInfo, SupportedToken } from "../lib/tauri";
import { signAndSendTransactions } from "../lib/transactions";
import {
  useWeb3Modal,
  useWeb3ModalAccount,
  useWeb3ModalProvider,
} from "@web3modal/ethers/react";
import { CATEGORIES_BY_TYPE } from "../lib/categories";
import type { ContentType } from "../lib/types";
import { useWalletStore } from "../store/walletStore";

type PublishStep = "form" | "importing" | "signing" | "confirming" | "adding-to-collection" | "done";

const STEP_LABELS: Record<PublishStep, string> = {
  form:                   "Publish Content",
  importing:              "Importing files into P2P network…",
  signing:                "Waiting for wallet approval…",
  confirming:             "Confirming and activating content…",
  "adding-to-collection": "Adding to collection…",
  done:                   "Published!",
};

function Publish() {
  const navigate = useNavigate();
  const { open: openModal } = useWeb3Modal();
  const { isConnected, address } = useWeb3ModalAccount();
  const { walletProvider } = useWeb3ModalProvider();
  const { araStaked } = useWalletStore();
  const needsStaking = isConnected && parseFloat(araStaked) < 10;

  const [title, setTitle] = useState("");
  const [description, setDescription] = useState("");
  const [priceEth, setPriceEth] = useState("");
  const [contentType, setContentType] = useState("game");
  const [selectedCategories, setSelectedCategories] = useState<string[]>([]);
  const [editionType, setEditionType] = useState<"unlimited" | "limited">("unlimited");
  const [maxCopies, setMaxCopies] = useState("");
  const [royaltyPercent, setRoyaltyPercent] = useState("10");
  const [filePath, setFilePath] = useState<string | null>(null);
  const [mainPreviewImagePath, setMainPreviewImagePath] = useState<string | null>(null);
  const [mainPreviewTrailerPath, setMainPreviewTrailerPath] = useState<string | null>(null);
  const [previewPaths, setPreviewPaths] = useState<string[]>([]);
  const [isDragging, setIsDragging] = useState(false);
  const [step, setStep] = useState<PublishStep>("form");
  const [error, setError] = useState<string | null>(null);
  const [resultHash, setResultHash] = useState<string | null>(null);

  // Payment token state
  const [paymentToken, setPaymentToken] = useState<string | null>(null); // null = ETH
  const [supportedTokens, setSupportedTokens] = useState<SupportedToken[]>([]);

  // Collaborator revenue splits
  const [splitMode, setSplitMode] = useState<"solo" | "split">("solo");
  const [collaborators, setCollaborators] = useState<{ wallet: string; percent: string }[]>([]);

  // Arweave permanent storage
  const [enableArweave, setEnableArweave] = useState(false);
  const [arweaveStatus, setArweaveStatus] = useState<string | null>(null);
  const unlistenRef = useRef<UnlistenFn | null>(null);

  // Listen for real-time Arweave progress events from the backend
  useEffect(() => {
    let cancelled = false;
    listen<{ step: string; detail: string }>("arweave-progress", (event) => {
      if (!cancelled) {
        setArweaveStatus(event.payload.detail);
      }
    }).then((unlisten) => {
      if (cancelled) {
        unlisten();
      } else {
        unlistenRef.current = unlisten;
      }
    });
    return () => {
      cancelled = true;
      unlistenRef.current?.();
      unlistenRef.current = null;
    };
  }, []);

  // Collection state
  const [collectionChoice, setCollectionChoice] = useState<"none" | "existing" | "new">("none");
  const [selectedCollectionId, setSelectedCollectionId] = useState<number | null>(null);
  const [collections, setCollections] = useState<CollectionInfo[]>([]);
  const [newCollectionName, setNewCollectionName] = useState("");
  const [newCollectionDescription, setNewCollectionDescription] = useState("");

  const isForm = step === "form";

  // Fetch user's collections on mount and when wallet connects
  useEffect(() => {
    if (isConnected) {
      getMyCollections().then(setCollections).catch(() => {});
    }
  }, [isConnected]);

  // Fetch supported tokens on mount
  useEffect(() => {
    getSupportedTokens().then(setSupportedTokens).catch(() => {});
  }, []);

  const selectFile = async () => {
    const selected = await open({ multiple: false, directory: false, title: "Select content file" });
    if (selected) {
      setFilePath(selected);
      if (!title.trim()) {
        const name = selected.split(/[\\/]/).pop()?.replace(/\.[^.]+$/, "") ?? "";
        if (name) setTitle(name);
      }
    }
  };

  const selectMainPreviewImage = async () => {
    const selected = await open({
      multiple: false, directory: false, title: "Select main preview image",
      filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "gif", "webp"] }],
    });
    if (selected) setMainPreviewImagePath(selected);
  };

  const selectMainPreviewTrailer = async () => {
    const selected = await open({
      multiple: false, directory: false, title: "Select main trailer video",
      filters: [{ name: "Videos", extensions: ["mp4", "webm", "mov"] }],
    });
    if (selected) setMainPreviewTrailerPath(selected);
  };

  const selectAdditionalPreviews = async () => {
    const selected = await open({
      multiple: true, directory: false, title: "Select additional preview images / videos",
      filters: [{ name: "Images & Videos", extensions: ["png", "jpg", "jpeg", "gif", "webp", "mp4", "webm", "mov"] }],
    });
    if (selected) {
      const paths = Array.isArray(selected) ? selected : [selected];
      setPreviewPaths((prev) => [...prev, ...paths]);
    }
  };

  const removeAdditionalPreview = (idx: number) => {
    setPreviewPaths((prev) => prev.filter((_, i) => i !== idx));
  };

  const activeCategories = CATEGORIES_BY_TYPE[contentType as ContentType] ?? CATEGORIES_BY_TYPE.other;

  // Clear selected categories when content type changes
  useEffect(() => {
    setSelectedCategories([]);
  }, [contentType]);

  const toggleCategory = (cat: string) => {
    setSelectedCategories((prev) =>
      prev.includes(cat) ? prev.filter((c) => c !== cat) : [...prev, cat]
    );
  };

  // Tauri v2 drag-drop: file drops from the OS are delivered via a window-level
  // event, not HTML5 drop events (v1 exposed .path on File — v2 removed it).
  useEffect(() => {
    let unlisten: UnlistenFn | null = null;
    let cancelled = false;
    getCurrentWebview()
      .onDragDropEvent((event) => {
        if (event.payload.type === "over" || event.payload.type === "enter") {
          setIsDragging(true);
        } else if (event.payload.type === "leave") {
          setIsDragging(false);
        } else if (event.payload.type === "drop") {
          setIsDragging(false);
          const paths = event.payload.paths;
          if (paths.length > 0) {
            const path = paths[0];
            setFilePath(path);
            setTitle((current) => {
              if (current.trim()) return current;
              const name = path.split(/[\\/]/).pop()?.replace(/\.[^.]+$/, "") ?? "";
              return name || current;
            });
          }
        }
      })
      .then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      });
    return () => {
      cancelled = true;
      if (unlisten) unlisten();
    };
  }, []);

  const fileName = filePath ? filePath.split(/[\\/]/).pop() ?? filePath : null;
  const splitsValid = splitMode === "solo" || (
    collaborators.length >= 2 &&
    collaborators.length <= 5 &&
    collaborators.every((c) => /^0x[a-fA-F0-9]{40}$/.test(c.wallet)) &&
    Math.abs(collaborators.reduce((s, c) => s + (parseFloat(c.percent) || 0), 0) - 100) < 0.01
  );
  const effectivePrice = priceEth.trim() === "" ? "0" : priceEth.trim();
  const priceValid = !isNaN(parseFloat(effectivePrice)) && parseFloat(effectivePrice) >= 0;
  const isFreePublish = parseFloat(effectivePrice) === 0;
  const canPublish = filePath && title.trim() && priceValid && isForm && splitsValid;

  const [showFreeConfirm, setShowFreeConfirm] = useState(false);

  const handlePublish = async () => {
    if (!filePath || !title.trim()) return;
    if (!isConnected) {
      openModal();
      return;
    }
    // Confirm free content to protect against forgetting to set a price
    if (isFreePublish && !showFreeConfirm) {
      setShowFreeConfirm(true);
      return;
    }
    setShowFreeConfirm(false);
    setError(null);
    setResultHash(null);
    try {
      // Resolve which collection ID to use (may require creating one first)
      let targetCollectionId: number | null = null;

      if (collectionChoice === "existing" && selectedCollectionId != null) {
        targetCollectionId = selectedCollectionId;
      } else if (collectionChoice === "new" && newCollectionName.trim()) {
        if (!walletProvider) {
          openModal();
          throw new Error("Wallet session expired — reconnect your wallet, then publish again.");
        }
        setStep("signing");
        const createTxs = await createCollection({
          name: newCollectionName.trim(),
          description: newCollectionDescription.trim(),
          bannerUri: "",
        });
        const createTxHash = await signAndSendTransactions(walletProvider, createTxs);
        setStep("confirming");
        targetCollectionId = await confirmCreateCollection({
          txHash: createTxHash,
          name: newCollectionName.trim(),
          description: newCollectionDescription.trim(),
          bannerUri: "",
        });
      }

      // Import files and prepare publish TX
      setStep("importing");
      const parsedMaxSupply = editionType === "limited" && maxCopies.trim()
        ? parseInt(maxCopies.trim(), 10)
        : undefined;
      const parsedRoyaltyBps = royaltyPercent.trim()
        ? Math.round(parseFloat(royaltyPercent.trim()) * 100)
        : undefined;
      // Build collaborator list (convert percentages to basis points)
      const collabInput = splitMode === "split" && collaborators.length > 0
        ? collaborators.map((c) => ({
            wallet: c.wallet.trim(),
            shareBps: Math.round(parseFloat(c.percent) * 100),
          }))
        : undefined;

      const result = await publishContent({
        filePath,
        title: title.trim(),
        description: description.trim(),
        contentType,
        priceEth: effectivePrice,
        maxSupply: parsedMaxSupply,
        royaltyBps: parsedRoyaltyBps,
        categories: selectedCategories.length > 0 ? selectedCategories : undefined,
        mainPreviewImagePath: mainPreviewImagePath ?? undefined,
        mainPreviewTrailerPath: mainPreviewTrailerPath ?? undefined,
        previewPaths: previewPaths.length > 0 ? previewPaths : undefined,
        paymentToken: paymentToken ?? undefined,
        collaborators: collabInput,
      });
      setResultHash(result.content_hash);

      // Sign publish TX and confirm
      let contentId: string;
      if (result.transactions.length > 0) {
        if (!walletProvider) {
          openModal();
          throw new Error("Wallet session expired — reconnect your wallet in the dialog that just opened, then publish again.");
        }
        setStep("signing");
        const txHash = await signAndSendTransactions(walletProvider, result.transactions);
        setStep("confirming");
        contentId = await confirmPublish(result.content_hash, txHash);
      } else {
        setStep("confirming");
        contentId = await confirmPublish(result.content_hash, "0x0");
      }

      // Arweave permanent storage (best-effort, non-fatal)
      // Backend emits "arweave-progress" events for real-time step updates.
      if (enableArweave && contentId) {
        try {
          if (!walletProvider) {
            setArweaveStatus("Arweave skipped: wallet disconnected");
          } else {
            setArweaveStatus("Preparing Arweave permanent storage...");
            const plan = await prepareArweaveUpload(contentId);
            setArweaveStatus(`Approve in wallet: fund Arweave storage (${plan.cost_eth} ETH)`);
            const fundTxHash = await signAndSendTransactions(walletProvider, plan.transactions);
            // Progress updates now come from backend events; final result updates here
            const arResult = await executeArweaveUpload(contentId, fundTxHash);
            setArweaveStatus(`Permanently stored: ${arResult.arweave_tx_id.slice(0, 12)}... (devnet.irys.xyz/${arResult.arweave_tx_id})`);
          }
        } catch (arErr: unknown) {
          const msg = arErr instanceof Error ? arErr.message
            : typeof arErr === "string" ? arErr
            : (arErr as { message?: string })?.message ?? JSON.stringify(arErr);
          setArweaveStatus(`Arweave upload failed: ${msg}`);
        }
      }

      // Add to collection if requested
      if (targetCollectionId != null) {
        try {
          setStep("adding-to-collection");
          if (!walletProvider) {
            throw new Error("Wallet disconnected");
          }
          const addTxs = await addToCollection(targetCollectionId, contentId);
          await signAndSendTransactions(walletProvider, addTxs);
          await confirmAddToCollection(targetCollectionId, contentId);
        } catch (collErr) {
          // Content is safely published — collection add failed non-fatally
          setError(`Published successfully, but failed to add to collection: ${collErr}. You can add it later from your Library.`);
          setStep("done");
          return;
        }
      }

      setStep("done");
      setTimeout(() => navigate("/library"), 1500);
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message
        : typeof err === "string" ? err
        : (err as { message?: string })?.message ?? JSON.stringify(err);
      setError(msg);
      setStep("form");
    }
  };

  return (
    <div className="max-w-2xl">
      <div className="mb-6">
        <h1 className="page-title">Publish Content</h1>
        <p className="page-subtitle">
          Share your content with the world. You'll earn 85% of every purchase.
        </p>
      </div>

      {!isConnected ? (
        <div className="card p-8 text-center">
          <p className="text-slate-400 dark:text-slate-500 mb-4">Connect your wallet to publish content.</p>
          <button onClick={() => openModal()} className="btn-primary px-6">
            Connect Wallet
          </button>
        </div>
      ) : needsStaking ? (
        <div className="card p-8 text-center">
          <p className="text-slate-400 dark:text-slate-500 mb-2">You need to stake at least 10 ARA before publishing.</p>
          <p className="text-xs text-slate-500 dark:text-slate-600 mb-4">The more you stake, the more you earn from network rewards.</p>
          <button onClick={() => navigate("/wallet")} className="btn-primary px-6">
            Go to Wallet to Stake
          </button>
        </div>
      ) : (

      <div className="space-y-5">
        {/* Content file drop zone */}
        <div>
          <label className="label">Content File</label>
          <button
            onClick={selectFile}
            disabled={!isForm}
            className={`w-full px-4 py-8 border-2 border-dashed rounded-xl transition-colors disabled:opacity-50 text-sm ${
              isDragging
                ? "border-ara-500 bg-ara-50 dark:bg-ara-950/20 text-ara-600"
                : filePath
                  ? "border-ara-500 dark:border-ara-700 text-ara-700 dark:text-ara-400 bg-ara-50 dark:bg-ara-950/20"
                  : "border-slate-300 dark:border-slate-700 text-slate-400 dark:text-slate-600 hover:border-ara-400 dark:hover:border-ara-700 hover:text-ara-600 dark:hover:text-ara-500"
            }`}
          >
            {filePath ? (
              <span className="flex flex-col items-center gap-1">
                <span className="font-medium text-base">{fileName}</span>
                <span className="text-xs opacity-70">Click to change file</span>
              </span>
            ) : (
              "Click to select file or drag and drop"
            )}
          </button>
        </div>

        {/* Title */}
        <div>
          <label className="label">Title</label>
          <input type="text" value={title} onChange={(e) => setTitle(e.target.value)}
            disabled={!isForm} placeholder="My Awesome Game" className="input-base" />
        </div>

        {/* Description */}
        <div>
          <label className="label">Description</label>
          <textarea value={description} onChange={(e) => setDescription(e.target.value)}
            disabled={!isForm} rows={3} placeholder="Describe your content…" className="input-base resize-none" />
        </div>

        {/* Type + Price row */}
        <div className="grid grid-cols-2 gap-4">
          <div>
            <label className="label">Content Type</label>
            <select value={contentType} onChange={(e) => setContentType(e.target.value)}
              disabled={!isForm} className="input-base">
              <option value="game">Game</option>
              <option value="music">Music</option>
              <option value="video">Video</option>
              <option value="document">Document</option>
              <option value="software">Software</option>
              <option value="other">Other</option>
            </select>
          </div>
          <div>
            <label className="label">
              Price ({paymentToken ? supportedTokens.find(t => t.address === paymentToken)?.symbol ?? "Token" : "ETH"})
            </label>
            <div className="flex gap-2">
              <input type="text" value={priceEth} onChange={(e) => setPriceEth(e.target.value)}
                disabled={!isForm} placeholder="0 for free, or 0.01" className="input-base flex-1" />
              <select
                value={paymentToken ?? ""}
                onChange={(e) => setPaymentToken(e.target.value || null)}
                disabled={!isForm}
                className="input-base w-auto text-sm"
              >
                <option value="">ETH</option>
                {supportedTokens.map((t) => (
                  <option key={t.address} value={t.address}>{t.symbol}</option>
                ))}
              </select>
            </div>
          </div>
        </div>

        {/* Categories */}
        <div>
          <label className="label">
            Categories{" "}
            <span className="text-slate-400 dark:text-slate-500 font-normal">(select all that apply)</span>
          </label>
          <div className="flex flex-wrap gap-1.5">
            {activeCategories.map((cat) => {
              const sel = selectedCategories.includes(cat);
              return (
                <button key={cat} type="button" onClick={() => toggleCategory(cat)}
                  disabled={!isForm}
                  className={`px-3 py-1 rounded-full text-xs font-medium border transition-colors disabled:opacity-50 ${
                    sel
                      ? "bg-ara-600 border-ara-600 text-white"
                      : "border-slate-300 dark:border-slate-700 text-slate-600 dark:text-slate-400 hover:border-ara-400 dark:hover:border-ara-600 bg-white dark:bg-slate-900"
                  }`}
                >
                  {cat}
                </button>
              );
            })}
          </div>
        </div>

        {/* Edition Settings */}
        <div className="card p-5 space-y-4">
          <div>
            <p className="text-sm font-semibold text-slate-900 dark:text-slate-100">Edition Settings</p>
            <p className="text-xs text-slate-500 dark:text-slate-500 mt-0.5">Control supply and earn royalties on resales</p>
          </div>

          <div>
            <label className="label-xs">Edition Type</label>
            <div className="flex gap-3">
              <button type="button" disabled={!isForm}
                onClick={() => setEditionType("unlimited")}
                className={`flex-1 px-4 py-2.5 rounded-lg text-sm font-medium border transition-colors disabled:opacity-50 ${
                  editionType === "unlimited"
                    ? "bg-ara-600 border-ara-600 text-white"
                    : "border-slate-300 dark:border-slate-700 text-slate-600 dark:text-slate-400 hover:border-ara-400 dark:hover:border-ara-600 bg-white dark:bg-slate-900"
                }`}>
                Unlimited
              </button>
              <button type="button" disabled={!isForm}
                onClick={() => setEditionType("limited")}
                className={`flex-1 px-4 py-2.5 rounded-lg text-sm font-medium border transition-colors disabled:opacity-50 ${
                  editionType === "limited"
                    ? "bg-ara-600 border-ara-600 text-white"
                    : "border-slate-300 dark:border-slate-700 text-slate-600 dark:text-slate-400 hover:border-ara-400 dark:hover:border-ara-600 bg-white dark:bg-slate-900"
                }`}>
                Limited Edition
              </button>
            </div>
          </div>

          {editionType === "limited" && (
            <div>
              <label className="label-xs">Max Copies</label>
              <input type="number" min="1" value={maxCopies}
                onChange={(e) => setMaxCopies(e.target.value)}
                disabled={!isForm} placeholder="e.g. 100"
                className="input-base w-40" />
            </div>
          )}

          <div>
            <label className="label-xs">Resale Royalty %</label>
            <div className="flex items-center gap-2">
              <input type="number" min="0" max="50" step="0.5" value={royaltyPercent}
                onChange={(e) => setRoyaltyPercent(e.target.value)}
                disabled={!isForm} placeholder="10"
                className="input-base w-24" />
              <span className="text-xs text-slate-500 dark:text-slate-500">% earned on every resale</span>
            </div>
          </div>
        </div>

        {/* Revenue Splits */}
        <div className="card p-5 space-y-4">
          <div>
            <p className="text-sm font-semibold text-slate-900 dark:text-slate-100">Revenue Splits</p>
            <p className="text-xs text-slate-500 dark:text-slate-500 mt-0.5">Split purchase revenue with collaborators. Splits are immutable after publishing.</p>
          </div>

          <div className="flex gap-2">
            <button type="button" disabled={!isForm}
              onClick={() => { setSplitMode("solo"); setCollaborators([]); }}
              className={`flex-1 px-4 py-2.5 rounded-lg text-sm font-medium border transition-colors disabled:opacity-50 ${
                splitMode === "solo"
                  ? "bg-ara-600 border-ara-600 text-white"
                  : "border-slate-300 dark:border-slate-700 text-slate-600 dark:text-slate-400 hover:border-ara-400 dark:hover:border-ara-600 bg-white dark:bg-slate-900"
              }`}>
              Solo
            </button>
            <button type="button" disabled={!isForm}
              onClick={() => {
                setSplitMode("split");
                if (collaborators.length === 0) {
                  setCollaborators([{ wallet: address ?? "", percent: "100" }]);
                }
              }}
              className={`flex-1 px-4 py-2.5 rounded-lg text-sm font-medium border transition-colors disabled:opacity-50 ${
                splitMode === "split"
                  ? "bg-ara-600 border-ara-600 text-white"
                  : "border-slate-300 dark:border-slate-700 text-slate-600 dark:text-slate-400 hover:border-ara-400 dark:hover:border-ara-600 bg-white dark:bg-slate-900"
              }`}>
              Split Revenue
            </button>
          </div>

          {splitMode === "split" && (
            <div className="space-y-3">
              {collaborators.map((collab, i) => (
                <div key={i} className="flex items-center gap-2">
                  <input
                    type="text"
                    value={collab.wallet}
                    onChange={(e) => {
                      const updated = [...collaborators];
                      updated[i] = { ...updated[i], wallet: e.target.value };
                      setCollaborators(updated);
                    }}
                    disabled={!isForm}
                    placeholder="0x..."
                    className={`input-base flex-1 font-mono text-xs ${
                      collab.wallet && !/^0x[a-fA-F0-9]{40}$/.test(collab.wallet)
                        ? "border-red-400 dark:border-red-600"
                        : ""
                    }`}
                  />
                  <div className="flex items-center gap-1">
                    <input
                      type="number"
                      min="0.01"
                      max="100"
                      step="0.01"
                      value={collab.percent}
                      onChange={(e) => {
                        const updated = [...collaborators];
                        updated[i] = { ...updated[i], percent: e.target.value };
                        setCollaborators(updated);
                      }}
                      disabled={!isForm}
                      className="input-base w-20 text-right"
                    />
                    <span className="text-xs text-slate-500">%</span>
                  </div>
                  {collaborators.length > 1 && (
                    <button type="button" disabled={!isForm}
                      onClick={() => setCollaborators(collaborators.filter((_, j) => j !== i))}
                      className="p-1.5 text-slate-400 hover:text-red-500 transition-colors disabled:opacity-50"
                      title="Remove collaborator">
                      <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
                        <path strokeLinecap="round" strokeLinejoin="round" d="M6 18L18 6M6 6l12 12" />
                      </svg>
                    </button>
                  )}
                </div>
              ))}

              {/* Validation feedback */}
              {(() => {
                const total = collaborators.reduce((sum, c) => sum + (parseFloat(c.percent) || 0), 0);
                const hasInvalidAddr = collaborators.some((c) => c.wallet && !/^0x[a-fA-F0-9]{40}$/.test(c.wallet));
                return (
                  <div className="flex items-center justify-between">
                    <div className="text-xs space-x-3">
                      <span className={Math.abs(total - 100) < 0.01 ? "text-green-600 dark:text-green-400" : "text-red-500"}>
                        Total: {total.toFixed(2)}%{Math.abs(total - 100) >= 0.01 && " (must equal 100%)"}
                      </span>
                      {hasInvalidAddr && (
                        <span className="text-red-500">Invalid address</span>
                      )}
                    </div>
                    <button type="button" disabled={!isForm || collaborators.length >= 5}
                      onClick={() => setCollaborators([...collaborators, { wallet: "", percent: "" }])}
                      className="text-xs text-ara-600 dark:text-ara-400 hover:text-ara-700 dark:hover:text-ara-300 font-medium disabled:opacity-40 disabled:cursor-not-allowed">
                      + Add Collaborator {collaborators.length >= 5 && "(max 5)"}
                    </button>
                  </div>
                );
              })()}
            </div>
          )}
        </div>

        {/* Preview assets */}
        <div className="card p-5 space-y-4">
          <div>
            <p className="text-sm font-semibold text-slate-900 dark:text-slate-100">Preview Assets</p>
            <p className="text-xs text-slate-500 dark:text-slate-500 mt-0.5">Optional — shown on your listing page</p>
          </div>

          {/* Main image */}
          <div>
            <label className="label-xs">Main Preview Image — listing cover</label>
            <div className="flex items-center gap-3">
              <button type="button" onClick={selectMainPreviewImage} disabled={!isForm}
                className="btn-secondary text-xs px-3 py-1.5">
                {mainPreviewImagePath ? "Change image" : "Select image"}
              </button>
              {mainPreviewImagePath && (
                <>
                  <span className="text-xs text-slate-600 dark:text-slate-400 truncate max-w-xs">
                    {mainPreviewImagePath.split(/[\\/]/).pop()}
                  </span>
                  <button type="button" onClick={() => setMainPreviewImagePath(null)}
                    disabled={!isForm} className="text-slate-400 hover:text-red-500 text-sm disabled:opacity-50">
                    ✕
                  </button>
                </>
              )}
            </div>
          </div>

          {/* Trailer */}
          <div>
            <label className="label-xs">Main Trailer Video — featured on detail page</label>
            <div className="flex items-center gap-3">
              <button type="button" onClick={selectMainPreviewTrailer} disabled={!isForm}
                className="btn-secondary text-xs px-3 py-1.5">
                {mainPreviewTrailerPath ? "Change trailer" : "Select trailer"}
              </button>
              {mainPreviewTrailerPath && (
                <>
                  <span className="text-xs text-slate-600 dark:text-slate-400 truncate max-w-xs">
                    {mainPreviewTrailerPath.split(/[\\/]/).pop()}
                  </span>
                  <button type="button" onClick={() => setMainPreviewTrailerPath(null)}
                    disabled={!isForm} className="text-slate-400 hover:text-red-500 text-sm disabled:opacity-50">
                    ✕
                  </button>
                </>
              )}
            </div>
          </div>

          {/* Additional */}
          <div>
            <label className="label-xs">Additional Screenshots &amp; Videos</label>
            <button type="button" onClick={selectAdditionalPreviews} disabled={!isForm}
              className="btn-secondary text-xs px-3 py-1.5">
              Add previews
            </button>
            {previewPaths.length > 0 && (
              <ul className="mt-2 space-y-1">
                {previewPaths.map((p, i) => (
                  <li key={i} className="flex items-center gap-2 text-xs text-slate-600 dark:text-slate-400">
                    <span className="truncate max-w-xs">{p.split(/[\\/]/).pop()}</span>
                    <button type="button" onClick={() => removeAdditionalPreview(i)}
                      disabled={!isForm} className="text-slate-400 hover:text-red-500 disabled:opacity-50 flex-shrink-0">
                      ✕
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </div>
        </div>

        {/* Permanent Storage (Arweave) */}
        <div className="card p-5">
          <div className="flex items-center justify-between">
            <div>
              <p className="text-sm font-semibold text-slate-900 dark:text-slate-100">Permanent Storage</p>
              <p className="text-xs text-slate-500 dark:text-slate-500 mt-0.5">
                Back up content to Arweave for permanent availability, even if all seeders go offline
              </p>
            </div>
            <button
              type="button"
              onClick={() => setEnableArweave(!enableArweave)}
              disabled={!isForm}
              className={`relative w-11 h-6 rounded-full transition-colors disabled:opacity-50 ${
                enableArweave
                  ? "bg-ara-600"
                  : "bg-slate-300 dark:bg-slate-700"
              }`}
            >
              <span className={`absolute top-0.5 left-0.5 w-5 h-5 bg-white rounded-full shadow transition-transform ${
                enableArweave ? "translate-x-5" : "translate-x-0"
              }`} />
            </button>
          </div>
          {enableArweave && (
            <p className="text-[10px] text-amber-600 dark:text-amber-400 mt-2">
              Arweave upload will run after publish. A small fee (paid in ETH to Irys) covers permanent storage.
              Cost scales with file size — not recommended for files over 100 MB.
            </p>
          )}
          {arweaveStatus && (
            <p className="text-[10px] text-slate-500 dark:text-slate-400 mt-1">{arweaveStatus}</p>
          )}
        </div>

        {/* Collection */}
        <div className="card p-5 space-y-4">
          <div>
            <p className="text-sm font-semibold text-slate-900 dark:text-slate-100">Collection</p>
            <p className="text-xs text-slate-500 dark:text-slate-500 mt-0.5">
              Optionally add this content to a collection
            </p>
          </div>

          <div className="flex gap-3">
            {(["none", "existing", "new"] as const).map((choice) => (
              <button key={choice} type="button" disabled={!isForm}
                onClick={() => { setCollectionChoice(choice); if (choice !== "existing") setSelectedCollectionId(null); }}
                className={`flex-1 px-4 py-2.5 rounded-lg text-sm font-medium border transition-colors disabled:opacity-50 ${
                  collectionChoice === choice
                    ? "bg-ara-600 border-ara-600 text-white"
                    : "border-slate-300 dark:border-slate-700 text-slate-600 dark:text-slate-400 hover:border-ara-400 dark:hover:border-ara-600 bg-white dark:bg-slate-900"
                }`}>
                {choice === "none" ? "None" : choice === "existing" ? "Existing" : "+ New"}
              </button>
            ))}
          </div>

          {collectionChoice === "existing" && (
            <div>
              {collections.length === 0 ? (
                <p className="text-xs text-slate-500 dark:text-slate-500">
                  No collections yet.{" "}
                  <button type="button" onClick={() => setCollectionChoice("new")}
                    className="text-ara-600 dark:text-ara-400 hover:underline">
                    Create one
                  </button>
                </p>
              ) : (
                <select value={selectedCollectionId ?? ""} disabled={!isForm}
                  onChange={(e) => setSelectedCollectionId(e.target.value ? Number(e.target.value) : null)}
                  className="input-base">
                  <option value="">Select a collection…</option>
                  {collections.map((c) => (
                    <option key={c.collection_id} value={c.collection_id}>
                      {c.name} ({c.item_count} items)
                    </option>
                  ))}
                </select>
              )}
            </div>
          )}

          {collectionChoice === "new" && (
            <div className="space-y-3">
              <div>
                <label className="label-xs">Collection Name</label>
                <input type="text" value={newCollectionName}
                  onChange={(e) => setNewCollectionName(e.target.value)}
                  disabled={!isForm} placeholder="My Collection"
                  className="input-base" />
              </div>
              <div>
                <label className="label-xs">Collection Description</label>
                <textarea value={newCollectionDescription}
                  onChange={(e) => setNewCollectionDescription(e.target.value)}
                  disabled={!isForm} rows={2} placeholder="Describe the collection…"
                  className="input-base resize-none" />
              </div>
            </div>
          )}
        </div>

        {/* Free content confirmation modal */}
        {showFreeConfirm && (
          <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm">
            <div className="card p-6 max-w-sm mx-4 shadow-2xl">
              <h3 className="text-lg font-bold text-slate-900 dark:text-slate-100 mb-2">Publish for free?</h3>
              <p className="text-sm text-slate-500 dark:text-slate-400 mb-1">
                Anyone will be able to download this content without paying.
              </p>
              <p className="text-xs text-slate-400 dark:text-slate-600 mb-5">
                Supporters can still tip you after downloading.
              </p>
              <div className="flex gap-3">
                <button
                  onClick={() => setShowFreeConfirm(false)}
                  className="btn-secondary flex-1"
                >
                  Cancel
                </button>
                <button
                  onClick={handlePublish}
                  className="btn-primary flex-1"
                >
                  Yes, Publish Free
                </button>
              </div>
            </div>
          </div>
        )}

        {error && <div className="alert-error">{error}</div>}

        {step === "done" && resultHash && (
          <div className="alert-success">
            <p className="font-medium">Published successfully!</p>
            <p className="mt-1 break-all text-xs opacity-80">Content hash: {resultHash}</p>
          </div>
        )}

        {/* Transaction count heads-up */}
        {isForm && canPublish && (enableArweave || collectionChoice !== "none") && (
          <div className="bg-slate-800/50 border border-slate-700 rounded-lg px-4 py-3">
            <p className="text-xs text-slate-300 font-medium">
              Wallet approvals needed:{" "}
              <span className="text-amber-400 font-bold">
                {1 + (enableArweave ? 1 : 0) + (collectionChoice !== "none" ? 1 : 0) + (collectionChoice === "new" ? 1 : 0)}
              </span>
            </p>
            <ul className="text-[10px] text-slate-400 mt-1 space-y-0.5 list-disc list-inside">
              {collectionChoice === "new" && <li>Create new collection</li>}
              <li>Publish content to blockchain</li>
              {enableArweave && <li>Fund Arweave permanent storage (small ETH fee)</li>}
              {collectionChoice !== "none" && <li>Add to collection</li>}
            </ul>
            <p className="text-[10px] text-slate-500 mt-1">
              Keep your wallet open — each step requires a separate approval.
            </p>
          </div>
        )}

        {step === "done" ? (
          <button onClick={() => { setStep("form"); setResultHash(null); setError(null); }}
            className="btn-primary-lg w-full">
            Publish Another
          </button>
        ) : (
          <button onClick={handlePublish} disabled={!canPublish} className="btn-primary-lg w-full">
            {STEP_LABELS[step]}
          </button>
        )}
      </div>

      )}
    </div>
  );
}

export default Publish;
