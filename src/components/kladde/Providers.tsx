import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { ExternalLink } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { apiGet, apiPost, apiDelete, errorDetail } from "@/lib/api";

type Language = "en" | "da";
type Status = {
  configured: boolean;
  mac: boolean;
  provider: Provider;
  model: string;
  account: string;
  codex_connected: boolean;
  /** Set once a key for that service is saved (Mac only). */
  gemini_model: string;
  openrouter_model: string;
  defaults: { gemini: string; openrouter: string };
  models?: string[];
};
type Provider = "cloudflare" | "codex" | "gemini" | "openrouter";
type KeyService = "gemini" | "openrouter";

export const providerName = (p?: Provider) =>
  ({ codex: "ChatGPT", gemini: "Gemini", openrouter: "OpenRouter", cloudflare: "Cloudflare" })[p ?? "cloudflare"];
export const useAiStatus = () =>
  useQuery({
    queryKey: ["ai-status"],
    queryFn: () => apiGet<Status>("/ai/status"),
    staleTime: Infinity,
  });

/** An outside link. "button" is a clear blue button for the main step; "text" is a guide link. */
function Link({ url, children, button = false }: { url: string; children: string; button?: boolean }) {
  return (
    <a
      className={
        button
          ? "inline-flex items-center gap-1.5 rounded-lg bg-blue-600 px-3 py-1.5 text-xs font-semibold text-white no-underline hover:bg-blue-700"
          : "inline-flex items-center gap-1 text-blue-700 underline underline-offset-2 hover:text-blue-900"
      }
      href={url}
      target="_blank"
      rel="noreferrer"
      data-testid="outside-link"
    >
      {children}
      <ExternalLink className="size-3.5" aria-hidden />
    </a>
  );
}

export function AiSetup({ lang }: { lang: Language }) {
  const da = lang === "da";
  const { data: s } = useAiStatus();
  const qc = useQueryClient();
  const [account, setAccount] = useState<string | null>(null);
  const [key, setKey] = useState("");
  const [models, setModels] = useState<string[]>([]);
  const id = (account ?? s?.account ?? "").trim();
  const update = useMutation({
    mutationFn: ({ action, model }: { action: string; model?: string }) => {
      if (action === "test")
        return apiPost<Status>("/ai/key", { account: id, key });
      if (action === "forget") return apiDelete<Status>("/ai/key", { provider: "cloudflare" });
      return apiPost<Status>("/ai/codex", { action, model });
    },
    meta: { errorShownInline: true },
    onSuccess: (result, { action }) => {
      qc.setQueryData(["ai-status"], result);
      if (result.models) setModels(result.models);
      if (action === "logout") setModels([]);
      setKey("");
      setAccount(null);
      toast.success(da ? "AI-indstillinger opdateret" : "AI settings updated");
    },
  });
  return (
    <section
      className="space-y-5 rounded-xl border border-blue-100 bg-white p-5 text-sm"
      data-testid="ai-setup-card"
    >
      {s?.configured && (
        <p className="rounded-lg bg-blue-50 px-3 py-2" data-testid="ai-active-provider">
          {da ? "I brug: " : "In use: "}
          <strong>{providerName(s.provider)}</strong> · {s.model}
        </p>
      )}
      <h2 className="font-heading text-lg font-semibold">
        Cloudflare Workers AI
      </h2>
      <details open={!s?.account}>
        <summary className="cursor-pointer font-medium">
          {da ? "Opsæt din gratis konto" : "Set up your free account"}
        </summary>
        <ol className="mt-3 list-decimal space-y-4 pl-6 text-slate-700">
          <li className="space-y-2">
            <p>
              {da
                ? "Opret en gratis Cloudflare-konto og bekræft din e-mail. Bliv på den gratis plan – du behøver hverken et domæne eller en Worker."
                : "Create a free Cloudflare account and verify your email. Stay on the free plan – you don't need a domain or a Worker."}
            </p>
            <Link button url="https://dash.cloudflare.com/sign-up">
              {da ? "Opret Cloudflare-konto" : "Create Cloudflare account"}
            </Link>
          </li>
          <li className="space-y-2">
            <p>
              {da
                ? "Åbn dashboardet og kopiér dit Account ID (32 tegn). Det står under kontonavnet, eller i adresselinjen efter dash.cloudflare.com/."
                : "Open the dashboard and copy your Account ID (32 characters). It is shown under the account name, or in the address bar after dash.cloudflare.com/."}
            </p>
            <div className="flex flex-wrap items-center gap-3">
              <Link button url="https://dash.cloudflare.com/">
                {da ? "Åbn Cloudflare-dashboardet" : "Open the Cloudflare dashboard"}
              </Link>
              <Link url="https://developers.cloudflare.com/fundamentals/account/find-account-and-zone-ids/">
                {da ? "Hvor finder jeg Account ID?" : "Where is my Account ID?"}
              </Link>
            </div>
          </li>
          <li className="space-y-2">
            <p>
              {da
                ? "Åbn Workers AI, vælg “Use REST API” og derefter “Create a Workers AI API Token”. Vælg din konto, opret tokenet, og kopiér det, mens det vises."
                : "Open Workers AI, choose “Use REST API” and then “Create a Workers AI API Token”. Select your account, create the token and copy it while it is shown."}
            </p>
            <div className="flex flex-wrap items-center gap-3">
              <Link button url="https://dash.cloudflare.com/?to=/:account/ai/workers-ai">
                {da ? "Åbn Workers AI" : "Open Workers AI"}
              </Link>
              <Link url="https://developers.cloudflare.com/workers-ai/get-started/rest-api/">
                {da ? "Cloudflares trin-for-trin-guide" : "Cloudflare’s walkthrough"}
              </Link>
            </div>
          </li>
          <li className="space-y-2">
            <p>
              {da
                ? "Laver du tokenet selv under API Tokens, så giv det adgang til din konto med Workers AI Read og Edit. Indsæt Account ID og token herunder."
                : "If you create the token yourself under API Tokens, give it access to your account with Workers AI Read and Edit. Paste the Account ID and token below."}
            </p>
            <Link url="https://dash.cloudflare.com/profile/api-tokens">
              {da ? "Administrer API-tokens" : "Manage API tokens"}
            </Link>
          </li>
        </ol>
      </details>
      <form
        className="space-y-3"
        onSubmit={(e) => {
          e.preventDefault();
          update.mutate({ action: "test" });
        }}
      >
        <label className="block">
          Account ID
          <Input
            required
            value={account ?? s?.account ?? ""}
            onChange={(e) => setAccount(e.target.value)}
            pattern="[a-fA-F0-9]{32}"
            spellCheck={false}
            autoComplete="off"
          />
        </label>
        <label className="block">
          API token
          <Input
            data-testid="ai-key-input"
            type="password"
            autoComplete="off"
            value={key}
            onChange={(e) => setKey(e.target.value)}
            placeholder={
              id && id === s?.account
                ? da
                  ? "Brug gemt token"
                  : "Use saved token"
                : ""
            }
          />
        </label>
        <div className="flex flex-wrap gap-2">
          <Button
            type="submit"
            data-testid="ai-key-save"
            disabled={
              update.isPending || !id || (!key.trim() && id !== s?.account)
            }
          >
            {da ? "Test og brug Cloudflare" : "Test and use Cloudflare"}
          </Button>
          {s?.account && (
            <Button
              type="button"
              variant="outline"
              disabled={update.isPending}
              onClick={() => update.mutate({ action: "forget" })}
            >
              {da ? "Fjern gemt token" : "Remove saved token"}
            </Button>
          )}
        </div>
      </form>
      <p className="text-xs text-slate-600">
        {da
          ? "Testen sender kun “Say OK”. Tokenet gemmes i computerens nøglelager. Den gratis kvote er 10.000 neuroner pr. konto pr. dag og nulstilles kl. 00.00 UTC. "
          : "The test sends only “Say OK”. The token is stored in your computer’s credential store. The free allowance is 10,000 neurons per account per day and resets at 00:00 UTC. "}
        <Link url="https://developers.cloudflare.com/workers-ai/platform/pricing/">
          {da ? "Se gældende grænser" : "View current limits"}
        </Link>
      </p>
      {s?.mac && (
        <>
          <KeySetup service="gemini" status={s} lang={lang} />
          <KeySetup service="openrouter" status={s} lang={lang} />
        </>
      )}
      {s?.mac && (
        <div className="space-y-3 border-t pt-4">
          <h2 className="font-heading text-lg font-semibold">
            ChatGPT · Codex
          </h2>
          <p>
            {da
              ? "Log ind i browseren med en ChatGPT-konto, der har adgang til Codex. Kladde bruger abonnementets modeller og forbrugsgrænser."
              : "Sign in through your browser with a ChatGPT account that includes Codex. Kladde uses the models and usage limits available to your subscription."}
          </p>
          <div className="flex flex-wrap items-center gap-2">
            <Button
              disabled={update.isPending}
              onClick={() => update.mutate({ action: "connect" })}
            >
              {da ? "Forbind og brug ChatGPT" : "Connect and use ChatGPT"}
            </Button>
            {s.provider === "codex" &&
              (models.length ? (
                <label>
                  Model{" "}
                  <select
                    className="rounded border p-2"
                    value={s.model}
                    disabled={update.isPending}
                    onChange={(e) =>
                      update.mutate({ action: "select", model: e.target.value })
                    }
                  >
                    {models.map((m) => (
                      <option key={m}>{m}</option>
                    ))}
                  </select>
                </label>
              ) : (
                <Button
                  variant="outline"
                  disabled={update.isPending}
                  onClick={() => update.mutate({ action: "models" })}
                >
                  {s.model} · {da ? "Skift model" : "Change model"}
                </Button>
              ))}
            {s.codex_connected && (
              <Button
                variant="outline"
                disabled={update.isPending}
                onClick={() => update.mutate({ action: "logout" })}
              >
                {da ? "Log ud" : "Sign out"}
              </Button>
            )}
          </div>
        </div>
      )}
      <p className="text-xs text-slate-600">
        {da
          ? "Den valgte tjeneste modtager tekst fra det AI-værktøj, du bruger. Chatten sender dit spørgsmål, tidligere beskeder i faget og dine lektier. Vælger du et fag, sendes også uddrag af fagets noter."
          : "Your selected provider receives the text used by each AI tool. Chat sends your question, recent subject messages and homework. Selecting a subject also sends excerpts from its notes."}
      </p>
      {update.isPending && (
        <p role="status">{da ? "Forbinder…" : "Connecting…"}</p>
      )}
      {update.isError && (
        <p role="alert" className="text-red-700">
          {errorDetail(
            update.error,
            da ? "Forbindelsen mislykkedes." : "Connection failed.",
          )}
        </p>
      )}
    </section>
  );
}

const keyText = {
  gemini: {
    title: "Google Gemini",
    steps: {
      da: "Log ind i Google AI Studio med en Google-konto, og vælg “Create API key”. Den gratis nøgle kræver intet betalingskort. Kopiér nøglen, og indsæt den herunder.",
      en: "Sign in to Google AI Studio with a Google account and choose “Create API key”. The free key needs no payment card. Copy the key and paste it below.",
    },
    signup: { url: "https://aistudio.google.com/apikey", da: "Opret Gemini-nøgle", en: "Create Gemini key" },
    guide: { url: "https://ai.google.dev/gemini-api/docs/rate-limits", da: "Se gratis grænser", en: "View free limits" },
    note: {
      da: "På den gratis plan kan Google bruge det, du sender, til at forbedre sine produkter. Send ikke personlige oplysninger.",
      en: "On the free tier, Google may use what you send to improve its products. Don't send personal information.",
    },
    suggestions: ["gemini-flash-latest", "gemini-flash-lite-latest", "gemini-pro-latest"],
  },
  openrouter: {
    title: "OpenRouter",
    steps: {
      da: "Opret en OpenRouter-konto, åbn “Keys”, og opret en nøgle. Modeller der ender på :free er gratis med en daglig grænse. Kopiér modellens ID fra modellisten.",
      en: "Create an OpenRouter account, open “Keys” and create a key. Models ending in :free cost nothing but have a daily limit. Copy the model ID from the model list.",
    },
    signup: { url: "https://openrouter.ai/settings/keys", da: "Opret OpenRouter-nøgle", en: "Create OpenRouter key" },
    guide: { url: "https://openrouter.ai/models?max_price=0", da: "Se gratis modeller", en: "Browse free models" },
    note: {
      da: "Gratis modeller kan gemme det, du sender, hos udbyderen. Send ikke personlige oplysninger.",
      en: "Free models may let the provider keep what you send. Don't send personal information.",
    },
    suggestions: ["nvidia/nemotron-3-ultra-550b-a55b:free", "nvidia/nemotron-3-ultra-550b-a55b"],
  },
} as const;

/** Setup for a service reached with one API key and a model ID (Gemini, OpenRouter). Mac only. */
function KeySetup({ service, status, lang }: { service: KeyService; status: Status; lang: Language }) {
  const da = lang === "da";
  const text = keyText[service];
  const qc = useQueryClient();
  const saved = status[`${service}_model`];
  const [key, setKey] = useState("");
  const [model, setModel] = useState<string | null>(null);
  const chosen = (model ?? (saved || status.defaults[service])).trim();
  const update = useMutation({
    mutationFn: (action: "test" | "forget") =>
      action === "test"
        ? apiPost<Status>("/ai/key", { provider: service, key, model: chosen })
        : apiDelete<Status>("/ai/key", { provider: service }),
    meta: { errorShownInline: true },
    onSuccess: (result) => {
      qc.setQueryData(["ai-status"], result);
      setKey("");
      setModel(null);
      toast.success(da ? "AI-indstillinger opdateret" : "AI settings updated");
    },
  });
  return (
    <div className="space-y-3 border-t pt-4" data-testid={`ai-setup-${service}`}>
      <h2 className="font-heading text-lg font-semibold">
        {text.title}
        {status.provider === service && (
          <span className="ml-2 rounded bg-blue-100 px-2 py-0.5 text-xs font-medium text-blue-800">
            {da ? "I brug" : "In use"}
          </span>
        )}
      </h2>
      <p className="text-slate-700">{text.steps[lang]}</p>
      <div className="flex flex-wrap items-center gap-3">
        <Link button url={text.signup.url}>{text.signup[lang]}</Link>
        <Link url={text.guide.url}>{text.guide[lang]}</Link>
      </div>
      <form
        className="space-y-3"
        onSubmit={(e) => {
          e.preventDefault();
          update.mutate("test");
        }}
      >
        <label className="block">
          API key
          <Input
            type="password"
            autoComplete="off"
            value={key}
            onChange={(e) => setKey(e.target.value)}
            placeholder={saved ? (da ? "Brug gemt nøgle" : "Use saved key") : ""}
          />
        </label>
        <label className="block">
          Model
          <Input
            value={model ?? (saved || status.defaults[service])}
            onChange={(e) => setModel(e.target.value)}
            list={`ai-models-${service}`}
            spellCheck={false}
            autoComplete="off"
          />
          <datalist id={`ai-models-${service}`}>
            {text.suggestions.map((m) => (
              <option key={m} value={m} />
            ))}
          </datalist>
        </label>
        <div className="flex flex-wrap gap-2">
          <Button type="submit" disabled={update.isPending || !chosen || (!key.trim() && !saved)}>
            {da ? `Test og brug ${text.title}` : `Test and use ${text.title}`}
          </Button>
          {saved && (
            <Button type="button" variant="outline" disabled={update.isPending} onClick={() => update.mutate("forget")}>
              {da ? "Fjern gemt nøgle" : "Remove saved key"}
            </Button>
          )}
        </div>
      </form>
      <p className="text-xs text-slate-600">
        {da
          ? "Testen sender kun “Say OK”. Nøglen gemmes i computerens nøglelager. "
          : "The test sends only “Say OK”. The key is stored in your computer’s credential store. "}
        {text.note[lang]}
      </p>
      {update.isPending && <p role="status">{da ? "Forbinder…" : "Connecting…"}</p>}
      {update.isError && (
        <p role="alert" className="text-red-700">
          {errorDetail(update.error, da ? "Forbindelsen mislykkedes." : "Connection failed.")}
        </p>
      )}
    </div>
  );
}

type Item = { id: string; title: string; subject: string };
type Inbox = {
  shared: string | null;
  pending: string[];
};

export function ClaudePanel({ lang }: { lang: Language }) {
  const da = lang === "da";
  const qc = useQueryClient();
  const [open, setOpen] = useState(false);
  const [selection, setSelection] = useState("");
  const dashboard = useQuery({
    queryKey: ["dashboard"],
    queryFn: () => apiGet<{ notes: Item[]; tasks: Item[] }>("/dashboard"),
    enabled: open,
  });
  const inbox = useQuery({
    queryKey: ["claude-inbox"],
    queryFn: () => apiPost<Inbox>("/claude", { action: "status" }),
    enabled: open,
    refetchInterval: open ? 5000 : false,
  });
  const [position, setPosition] = useState(0);
  const ids = inbox.data?.pending ?? [];
  const index = Math.max(0, Math.min(position, ids.length - 1));
  const draftId = ids[index];
  const preview = useQuery({
    queryKey: ["claude-draft", draftId],
    queryFn: () => apiPost<Item & { kind: string; content?: string; deadline?: string }>("/claude", { action: "preview", id: draftId }),
    enabled: open && Boolean(draftId),
    staleTime: Infinity,
    retry: false,
  });
  const update = useMutation({
    mutationFn: (args: { action: string; id?: string; kind?: string }) =>
      apiPost<Inbox>("/claude", args),
    meta: { errorShownInline: true },
    onSuccess: (result, args) => {
      qc.setQueryData(["claude-inbox"], result);
      if (args.action === "install")
        toast.success(
          da
            ? "Luk Claude Desktop helt, og åbn det igen."
            : "Quit and reopen Claude Desktop.",
        );
      if (args.action === "accept") {
        void qc.invalidateQueries({ queryKey: ["dashboard"] });
        void qc.invalidateQueries({ queryKey: ["weekly-summary"] });
      }
    },
  });
  return (
    <details
      className="space-y-3 rounded-xl border bg-white p-4 text-sm"
      onToggle={(e) => setOpen(e.currentTarget.open)}
    >
      <summary className="cursor-pointer font-medium">
        Claude Desktop ·{" "}
        {da ? "Del en note eller lektie" : "Share a note or homework"}
      </summary>
      <ol className="list-decimal space-y-2 pl-6">
        <li>
          {da
            ? "Flyt Kladde til Programmer. Installér "
            : "Move Kladde to Applications. Install "}
          <Link url="https://claude.com/download">Claude Desktop</Link>
          {da
            ? " og log ind med din Claude-konto."
            : " and sign in with your Claude account."}
        </li>
        <li>
          {da
            ? "Føj Kladde til Claude Desktops MCP-indstillinger med knappen herunder. Luk derefter Claude helt med ⌘Q og åbn det igen."
            : "Use the button below to add Kladde to Claude Desktop’s MCP settings. Quit Claude with ⌘Q and reopen it."}
        </li>
        <li>
          {da
            ? "Del et emne nedenfor. Bed Claude i Claude Desktop om at læse det via Kladde og foreslå et udkast. Tillad værktøjet, når Claude spørger. Gennemse udkastet her."
            : "Share an item below. In Claude Desktop, ask Claude to read it using Kladde and suggest a draft. Allow the tool when prompted, then review the draft here."}
        </li>
      </ol>
      <Button
        variant="outline"
        disabled={update.isPending}
        onClick={() => update.mutate({ action: "install" })}
      >
        {da ? "Føj Kladde til Claude Desktop" : "Add Kladde to Claude Desktop"}
      </Button>
      <div className="flex flex-wrap gap-2">
        <select
          className="max-w-full rounded border p-2"
          aria-label={da ? "Vælg emne" : "Select item"}
          value={selection}
          onChange={(e) => setSelection(e.target.value)}
        >
          <option value="">
            {da ? "Vælg en note eller lektie" : "Choose a note or homework"}
          </option>
          {(["notes", "tasks"] as const).map((group) => (
            <optgroup
              key={group}
              label={
                group === "notes"
                  ? da
                    ? "Noter"
                    : "Notes"
                  : da
                    ? "Lektier"
                    : "Homework"
              }
            >
              {dashboard.data?.[group].map((i) => (
                <option key={i.id} value={group + "|" + i.id}>
                  {i.subject} · {i.title}
                </option>
              ))}
            </optgroup>
          ))}
        </select>
        <Button
          disabled={!selection || update.isPending}
          onClick={() => {
            const [group, id] = selection.split("|");
            update.mutate({
              action: "share",
              kind: group === "notes" ? "note" : "task",
              id,
            });
          }}
        >
          {da ? "Del med Claude" : "Share with Claude"}
        </Button>
      </div>
      {inbox.data?.shared && (
        <p>
          {da ? "Delt kopi" : "Shared snapshot"} · {inbox.data.shared}{" "}
          <Button
            variant="outline"
            disabled={update.isPending}
            onClick={() => update.mutate({ action: "revoke" })}
          >
            {da ? "Stop deling" : "Stop sharing"}
          </Button>
        </p>
      )}
      {draftId && (
        <article className="space-y-2 rounded border p-3">
          <div className="flex items-center gap-2">
            <Button variant="outline" size="sm" disabled={index === 0} onClick={() => setPosition(index - 1)}>←</Button>
            <span>{da ? "Udkast" : "Draft"} {index + 1} / {ids.length}</span>
            <Button variant="outline" size="sm" disabled={index + 1 >= ids.length} onClick={() => setPosition(index + 1)}>→</Button>
          </div>
          {preview.data && <>
            <h3 className="font-semibold">{preview.data.title}</h3>
            <p>{preview.data.subject} · {preview.data.kind === "note" ? "Note" : da ? "Lektie" : "Homework"} {preview.data.deadline}</p>
            {preview.data.content && <p className="max-h-60 overflow-auto whitespace-pre-wrap">{preview.data.content}</p>}
          </>}
          {preview.isError && <p role="alert">{da ? "Udkastet kunne ikke læses." : "Could not read this draft."}</p>}
          <div className="flex gap-2">
            <Button disabled={update.isPending || !preview.data} onClick={() => update.mutate({ action: "accept", id: draftId })}>
              {da ? "Gem som ny" : "Save as new"}
            </Button>
            <Button variant="outline" disabled={update.isPending} onClick={() => update.mutate({ action: "discard", id: draftId })}>
              {da ? "Kassér" : "Discard"}
            </Button>
          </div>
        </article>
      )}
      {(update.isError || inbox.isError) && (
        <p role="alert" className="text-red-700">
          {errorDetail(
            update.error || inbox.error,
            da
              ? "Claude-forbindelsen mislykkedes."
              : "Claude connection failed.",
          )}
        </p>
      )}
    </details>
  );
}
