import { useState, useEffect, useCallback, type DragEvent } from "react";
import { useNavigate } from "react-router-dom";
import { open } from "@tauri-apps/plugin-dialog";
import { publishContent, confirmPublish } from "../lib/tauri";
import { signAndSendTransactions } from "../lib/transactions";
import {
  useWeb3Modal,
  useWeb3ModalAccount,
  useWeb3ModalProvider,
} from "@web3modal/ethers/react";
import { CATEGORIES_BY_TYPE } from "../lib/categories";
import type { ContentType } from "../lib/types";

type PublishStep = "form" | "importing" | "signing" | "confirming" | "done";

const STEP_LABELS: Record<PublishStep, string> = {
  form:       "Publish Content",
  importing:  "Importing files into P2P network…",
  signing:    "Waiting for wallet approval…",
  confirming: "Confirming and activating content…",
  done:       "Published!",
};

function Publish() {
  const navigate = useNavigate();
  const { open: openModal } = useWeb3Modal();
  const { isConnected } = useWeb3ModalAccount();
  const { walletProvider } = useWeb3ModalProvider();

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

  const isForm = step === "form";

  const selectFile = async () => {
    const selected = await open({ multiple: false, directory: false, title: "Select content file" });
    if (selected) setFilePath(selected);
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

  const handleDragOver = useCallback((e: DragEvent) => {
    e.preventDefault(); e.stopPropagation(); setIsDragging(true);
  }, []);
  const handleDragLeave = useCallback((e: DragEvent) => {
    e.preventDefault(); e.stopPropagation(); setIsDragging(false);
  }, []);
  const handleDrop = useCallback((e: DragEvent) => {
    e.preventDefault(); e.stopPropagation(); setIsDragging(false);
    const files = e.dataTransfer.files;
    if (files.length > 0) {
      const path = (files[0] as unknown as { path?: string }).path;
      if (path) setFilePath(path);
    }
  }, []);

  const fileName = filePath ? filePath.split(/[\\/]/).pop() ?? filePath : null;
  const canPublish = filePath && title.trim() && priceEth.trim() && isForm;

  const handlePublish = async () => {
    if (!filePath || !title.trim() || !priceEth.trim()) return;
    if (!isConnected) {
      openModal();
      return;
    }
    setError(null);
    setResultHash(null);
    try {
      setStep("importing");
      const parsedMaxSupply = editionType === "limited" && maxCopies.trim()
        ? parseInt(maxCopies.trim(), 10)
        : undefined;
      const parsedRoyaltyBps = royaltyPercent.trim()
        ? Math.round(parseFloat(royaltyPercent.trim()) * 100)
        : undefined;
      const result = await publishContent({
        filePath,
        title: title.trim(),
        description: description.trim(),
        contentType,
        priceEth: priceEth.trim(),
        maxSupply: parsedMaxSupply,
        royaltyBps: parsedRoyaltyBps,
        categories: selectedCategories.length > 0 ? selectedCategories : undefined,
        mainPreviewImagePath: mainPreviewImagePath ?? undefined,
        mainPreviewTrailerPath: mainPreviewTrailerPath ?? undefined,
        previewPaths: previewPaths.length > 0 ? previewPaths : undefined,
      });
      setResultHash(result.content_hash);
      if (result.transactions.length > 0) {
        if (!walletProvider) {
          openModal();
          throw new Error("Wallet session expired — reconnect your wallet in the dialog that just opened, then publish again.");
        }
        setStep("signing");
        const txHash = await signAndSendTransactions(walletProvider, result.transactions);
        setStep("confirming");
        await confirmPublish(result.content_hash, txHash);
      } else {
        setStep("confirming");
        await confirmPublish(result.content_hash, "0x0");
      }
      setStep("done");
      setTimeout(() => navigate("/library"), 1500);
    } catch (err) {
      setError(String(err));
      setStep("form");
    }
  };

  return (
    <div className="max-w-2xl">
      <div className="mb-6">
        <h1 className="page-title">Publish Content</h1>
        <p className="page-subtitle">
          Share your content with the world. You'll earn 85% of every purchase in ETH.
        </p>
      </div>

      <div className="space-y-5">
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
            <label className="label">Price (ETH)</label>
            <input type="text" value={priceEth} onChange={(e) => setPriceEth(e.target.value)}
              disabled={!isForm} placeholder="0.01" className="input-base" />
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

        {/* Content file drop zone */}
        <div>
          <label className="label">Content File</label>
          <button
            onClick={selectFile}
            onDragOver={handleDragOver}
            onDragLeave={handleDragLeave}
            onDrop={handleDrop}
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

        {error && <div className="alert-error">{error}</div>}

        {step === "done" && resultHash && (
          <div className="alert-success">
            <p className="font-medium">Published successfully!</p>
            <p className="mt-1 break-all text-xs opacity-80">Content hash: {resultHash}</p>
          </div>
        )}

        {step === "done" ? (
          <button onClick={() => { setStep("form"); setResultHash(null); setError(null); }}
            className="btn-primary-lg w-full">
            Publish Another
          </button>
        ) : (
          <button onClick={handlePublish} disabled={!canPublish} className="btn-primary-lg w-full">
            {!isConnected && isForm ? "Connect Wallet to Publish" : STEP_LABELS[step]}
          </button>
        )}
      </div>
    </div>
  );
}

export default Publish;
