// Updates: Kladde checks GitHub for a newer signed version when it starts and offers to
// install it. Nothing is installed without the user's click, and the study data (in its own
// data folder) is never touched by an update.
import { useEffect, useState } from "react";
import { toast } from "sonner";
import { Download, RefreshCw, X } from "lucide-react";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { Button } from "@/components/ui/button";

type Language = "da" | "en";
type Listener = (u: Update | null) => void;

let found: Update | null = null;
const listeners = new Set<Listener>();
const announce = (u: Update | null) => {
  found = u;
  listeners.forEach((l) => l(u));
};

/** Asks GitHub whether a newer version exists. Returns the update or null. */
async function lookForUpdate(): Promise<Update | null> {
  // A time limit, so a bad connection gives a clear message instead of an endless spinner.
  const u = await check({ timeout: 20_000 });
  announce(u);
  return u;
}

function useUpdate() {
  const [u, setU] = useState<Update | null>(found);
  useEffect(() => {
    listeners.add(setU);
    return () => void listeners.delete(setU);
  }, []);
  return u;
}

/** Banner shown above the page when a new version is ready. */
export function UpdateBanner({ lang }: { lang: Language }) {
  const da = lang === "da";
  const update = useUpdate();
  const [hidden, setHidden] = useState(false);
  const [progress, setProgress] = useState<number | null>(null);
  useEffect(() => {
    // Check a few seconds after start; being offline is not an error worth showing.
    const t = window.setTimeout(() => lookForUpdate().catch(() => {}), 4000);
    return () => window.clearTimeout(t);
  }, []);
  if (!update || hidden) return null;
  const install = async () => {
    let total = 0;
    let done = 0;
    setProgress(0);
    try {
      await update.downloadAndInstall((event) => {
        if (event.event === "Started") total = event.data.contentLength ?? 0;
        if (event.event === "Progress") {
          done += event.data.chunkLength;
          setProgress(total ? Math.min(99, Math.round((done / total) * 100)) : 0);
        }
      });
      // On Windows the installer closes and reopens Kladde itself; elsewhere restart here.
      await relaunch();
    } catch (e) {
      setProgress(null);
      toast.error(da ? "Opdateringen kunne ikke installeres. Prøv igen senere." : "The update could not be installed. Try again later.", {
        description: String(e),
      });
    }
  };
  return (
    <div
      className="mb-5 flex flex-wrap items-center gap-3 rounded-2xl border border-blue-200 bg-blue-50 p-4 text-sm text-blue-900"
      role="status"
      data-testid="update-banner"
    >
      <Download className="size-4 shrink-0 text-blue-600" />
      <span className="min-w-0 flex-1">
        <span className="font-semibold">
          {da ? `Kladde ${update.version} er klar.` : `Kladde ${update.version} is ready.`}
        </span>{" "}
        {progress !== null
          ? da
            ? `Henter og installerer… ${progress ? `${progress} %` : ""}`
            : `Downloading and installing… ${progress ? `${progress}%` : ""}`
          : da
            ? "Dine noter og lektier bliver, hvor de er."
            : "Your notes and homework stay where they are."}
      </span>
      {progress === null && (
        <>
          <Button size="sm" className="bg-blue-600 hover:bg-blue-700" onClick={install} data-testid="update-install">
            <RefreshCw className="size-3.5" /> {da ? "Opdatér og genstart" : "Update and restart"}
          </Button>
          <Button size="sm" variant="ghost" onClick={() => setHidden(true)} title={da ? "Senere" : "Later"}>
            <X className="size-4" />
          </Button>
        </>
      )}
    </div>
  );
}

/** "Check for updates" link for the footer. */
export function CheckForUpdates({ lang }: { lang: Language }) {
  const da = lang === "da";
  const [busy, setBusy] = useState(false);
  return (
    <button
      disabled={busy}
      onClick={async () => {
        setBusy(true);
        try {
          const u = await lookForUpdate();
          if (!u) toast.success(da ? "Du har den nyeste version af Kladde." : "You have the latest version of Kladde.");
        } catch (e) {
          toast.error(da ? "Kunne ikke søge efter opdateringer. Tjek internetforbindelsen." : "Could not check for updates. Check your connection.", {
            description: String(e),
          });
        } finally {
          setBusy(false);
        }
      }}
      className="flex items-center gap-1 font-semibold text-slate-500 hover:text-blue-600 disabled:opacity-50"
      data-testid="update-check"
    >
      <RefreshCw className={`size-3.5 ${busy ? "animate-spin" : ""}`} />
      {da ? "Søg efter opdateringer" : "Check for updates"}
    </button>
  );
}
