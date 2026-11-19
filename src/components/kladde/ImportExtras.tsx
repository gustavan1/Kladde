import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import {
  CalendarDays,
  ClipboardCopy,
  Link2,
  MousePointerClick,
  RefreshCw,
  Trash2,
  Upload,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { apiDelete, apiGet, apiPost, errorDetail } from "@/lib/api";

type Language = "da" | "en";

// Mirrors the calendar feed routes in server/index.ts
export interface CalendarFeed {
  id: string;
  label: string;
  url_preview: string;
  needs_url: boolean;
  created_at: string;
  last_synced_at: string | null;
  last_status: string;
  last_error: string | null;
  imported_tasks: number;
  imported_blocks: number;
}
export interface CalendarSyncResult {
  feed: CalendarFeed;
  new_tasks: number;
  new_blocks: number;
}

const errorText = errorDetail;

export function CalendarLinks({ lang, refresh }: { lang: Language; refresh: () => void }) {
  const da = lang === "da";
  const qc = useQueryClient();
  const [url, setUrl] = useState("");
  const [label, setLabel] = useState("");
  const [reconnect, setReconnect] = useState<CalendarFeed | null>(null);
  const feeds = useQuery({
    queryKey: ["calendar-feeds"],
    queryFn: () => apiGet<CalendarFeed[]>("/calendar-feeds"),
    retry: false,
  });
  const done = (result: CalendarSyncResult) => {
    void qc.invalidateQueries({ queryKey: ["calendar-feeds"] });
    refresh();
    if (result.feed.last_status === "error")
      toast.error(
        result.feed.last_error || (da ? "Kalenderen kunne ikke hentes" : "Calendar could not be fetched"),
      );
    else
      toast.success(
        da
          ? `${result.new_tasks} deadlines og ${result.new_blocks} lektioner tilføjet`
          : `${result.new_tasks} deadlines and ${result.new_blocks} lessons added`,
      );
  };
  const add = useMutation({
    mutationFn: () =>
      apiPost<CalendarSyncResult>("/calendar-feeds", {
        url,
        reconnect_id: reconnect?.id,
        label: label || (da ? "Skolekalender" : "School calendar"),
      }),
    onSuccess: (r) => {
      setUrl("");
      setLabel("");
      setReconnect(null);
      done(r);
    },
    onError: (e) => toast.error(errorText(e, da ? "Linket kunne ikke tilføjes" : "Link could not be added")),
  });
  const sync = useMutation({
    mutationFn: (id: string) => apiPost<CalendarSyncResult>(`/calendar-feeds/${id}/sync`),
    onSuccess: done,
    onError: (e) => toast.error(errorText(e, da ? "Kalenderen kunne ikke opdateres" : "Calendar could not be synced")),
  });
  const remove = useMutation({
    mutationFn: (id: string) => apiDelete<{ deleted: boolean }>(`/calendar-feeds/${id}`),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["calendar-feeds"] });
      toast.success(da ? "Kalenderlink fjernet" : "Calendar link removed");
    },
  });
  const when = (iso: string | null) =>
    iso
      ? new Intl.DateTimeFormat(da ? "da-DK" : "en-GB", {
          day: "numeric",
          month: "short",
          hour: "2-digit",
          minute: "2-digit",
        }).format(new Date(iso))
      : "—";
  return (
    <Card className="border-slate-200/80 shadow-sm" data-testid="calendar-links-card">
      <CardHeader>
        <CardTitle className="flex items-center gap-2 font-heading text-lg">
          <Link2 className="size-4 text-blue-600" />{" "}
          {da ? "Kalenderlink (automatisk)" : "Calendar link (automatic)"}
        </CardTitle>
        <p className="text-xs leading-relaxed text-slate-500">
          {da
            ? "Indsæt et kalender-abonnementslink (.ics eller webcal://). Mens Kladde kører, tjekkes det hver 3. time for nye deadlines og lektioner."
            : "Paste a calendar subscription link (.ics or webcal://). While Kladde is running, it is checked every 3 hours for new deadlines and lessons."}
        </p>
      </CardHeader>
      <CardContent className="space-y-4">
        <div className="grid gap-2 sm:grid-cols-[1fr_180px_auto]">
          <Input
            value={url}
            onChange={(e) => setUrl(e.target.value)}
            placeholder="webcal://… eller https://….ics"
            className="h-10 rounded-xl"
            data-testid="calendar-url-input"
          />
          <Input
            disabled={Boolean(reconnect)}
            value={label}
            onChange={(e) => setLabel(e.target.value)}
            placeholder={da ? "Navn, fx Lectio" : "Name, e.g. Lectio"}
            className="h-10 rounded-xl"
            data-testid="calendar-label-input"
          />
          <Button
            onClick={() => add.mutate()}
            disabled={url.trim().length < 8 || add.isPending}
            className="h-10 bg-blue-600 hover:bg-blue-700"
            data-testid="calendar-add-button"
          >
            {add.isPending ? (da ? "Henter…" : "Fetching…") : reconnect ? (da ? "Tilslut igen" : "Reconnect") : da ? "Tilføj" : "Add"}
          </Button>
        </div>
        {reconnect && <button type="button" onClick={() => { setReconnect(null); setUrl(""); setLabel(""); }}
          className="text-xs underline">{da ? "Annuller gentilslutning" : "Cancel reconnection"}</button>}
        <div className="space-y-2" data-testid="calendar-feed-list">
          {(feeds.data || []).map((feed) => (
            <div
              key={feed.id}
              className="flex flex-wrap items-center gap-3 rounded-xl border border-slate-100 bg-slate-50/60 p-3"
              data-testid="calendar-feed-item"
            >
              <span
                className={`size-2 rounded-full ${feed.last_status === "ok" ? "bg-emerald-500" : feed.last_status === "error" ? "bg-red-500" : "bg-amber-400"}`}
              />
              <div className="min-w-0 flex-1">
                <p className="text-sm font-semibold text-slate-800" data-testid="calendar-feed-label">
                  {feed.label}
                </p>
                <p className="truncate text-xs text-slate-400">
                  {feed.url_preview} · {da ? "senest" : "last"} {when(feed.last_synced_at)} ·{" "}
                  {feed.imported_tasks} deadlines · {feed.imported_blocks} {da ? "lektioner" : "lessons"}
                </p>
                {feed.last_error && (
                  <p className="mt-1 text-xs text-red-600" data-testid="calendar-feed-error">
                    {feed.last_error}
                  </p>
                )}
              </div>
              <Button
                variant="outline"
                size="sm"
                onClick={() => {
                  if (feed.needs_url) { setReconnect(feed); setUrl(""); setLabel(feed.label); }
                  else sync.mutate(feed.id);
                }}
                disabled={sync.isPending}
                data-testid="calendar-sync-button"
              >
                <RefreshCw className={`size-3.5 ${sync.isPending ? "animate-spin" : ""}`} />{" "}
                {feed.needs_url ? (da ? "Tilslut link igen" : "Reconnect link") : da ? "Opdatér nu" : "Sync now"}
              </Button>
              <Button
                variant="ghost"
                size="icon-sm"
                onClick={() => remove.mutate(feed.id)}
                title={da ? "Fjern" : "Remove"}
                data-testid="calendar-delete-button"
              >
                <Trash2 className="size-3.5 text-slate-400" />
              </Button>
            </div>
          ))}
          {feeds.data?.length === 0 && (
            <p className="text-xs text-slate-400" data-testid="calendar-feed-empty">
              {da ? "Ingen kalenderlinks endnu." : "No calendar links yet."}
            </p>
          )}
        </div>
      </CardContent>
    </Card>
  );
}

type Step = { icon: typeof Upload; title: string; text: string; mock: string[] };
type GuideKey = "lectio" | "mitnielsbrock" | "ludus";

const guides = (da: boolean): Record<GuideKey, { name: string; steps: Step[]; tip: string }> => ({
  lectio: {
    name: "Lectio",
    steps: [
      {
        icon: MousePointerClick,
        title: da ? "Åbn Opgaver" : "Open Assignments",
        text: da
          ? "Log ind på Lectio og vælg 'Opgaver' i menuen."
          : "Sign in to Lectio and choose 'Opgaver' in the menu.",
        mock: ["Forside", "Skema", "▸ Opgaver", "Beskeder"],
      },
      {
        icon: ClipboardCopy,
        title: da ? "Kopiér tabellen" : "Copy the table",
        text: da
          ? "Markér hele opgavetabellen med musen (fra overskriften og ned) og tryk Ctrl/Cmd + C."
          : "Select the whole assignment table with the mouse (from the header down) and press Ctrl/Cmd + C.",
        mock: ["Hold | Opgavetitel | Frist", "Dansk | Analyse | 14/9-2026", "Mat | Aflevering 3 | 18/9-2026"],
      },
      {
        icon: Upload,
        title: da ? "Indsæt i Kladde" : "Paste into Kladde",
        text: da
          ? "Vælg 'Lectio' herover, indsæt i feltet og tryk 'Forhåndsvis import'."
          : "Choose 'Lectio' above, paste into the field and press 'Preview import'.",
        mock: ["Kilde: Lectio", "[ indsat tekst ]", "▸ Forhåndsvis import"],
      },
    ],
    tip: da
      ? "Vil du have det automatisk? Lectio har ikke selv et kalenderlink, men tjenester som LectioKalender kan give dig et webcal-link, som du indsætter under 'Kalenderlink'. Tjek din skoles regler, før du giver dit login til en tredjepart."
      : "Want it automatic? Lectio has no calendar link itself, but services like LectioKalender can give you a webcal link to paste under 'Calendar link'. Check your school's rules before giving your login to a third party.",
  },
  mitnielsbrock: {
    name: "Mit Niels Brock",
    steps: [
      {
        icon: MousePointerClick,
        title: da ? "Find dine frister" : "Find your deadlines",
        text: da
          ? "Log ind på Mit Niels Brock og åbn siden med dit skema eller afleveringsfrister."
          : "Sign in to Mit Niels Brock and open the page with your timetable or assessment deadlines.",
        mock: ["Mit Niels Brock", "▸ Skema", "▸ Afleveringer", "Materialer"],
      },
      {
        icon: CalendarDays,
        title: da ? "Kalenderlink eller kopi" : "Calendar link or copy",
        text: da
          ? "Finder du 'Abonnér', 'Eksportér' eller 'iCal', så kopiér linket. Ellers markér listen og kopiér den."
          : "If you see 'Subscribe', 'Export' or 'iCal', copy that link. Otherwise select the list and copy it.",
        mock: ["⋯  Eksportér / iCal", "webcal://…", "eller Ctrl/Cmd + C"],
      },
      {
        icon: Upload,
        title: da ? "Tilføj i Kladde" : "Add to Kladde",
        text: da
          ? "Et link indsættes under 'Kalenderlink'. Kopieret tekst indsættes i importfeltet med kilden 'Mit Niels Brock'."
          : "Paste a link under 'Calendar link'. Paste copied text into the import field with source 'Mit Niels Brock'.",
        mock: ["Kalenderlink → Tilføj", "eller", "Import → Forhåndsvis"],
      },
    ],
    tip: da
      ? "Menunavnene kan variere mellem uddannelser. Kan du ikke finde en eksport, så kontakt IT-helpdesk (helpdesk@nielsbrock.dk)."
      : "Menu names can vary between programmes. If you can't find an export, contact the IT helpdesk (helpdesk@nielsbrock.dk).",
  },
  ludus: {
    name: "Ludus · Niels Brock",
    steps: [
      {
        icon: MousePointerClick,
        title: da ? "Hent en CSV-fil" : "Get a CSV file",
        text: da
          ? "Ludus-udtræk laves typisk af skolen som CSV-filer. Spørg din lærer eller studieadministration om dit skema/hold som CSV."
          : "Ludus exports are usually made by the school as CSV files. Ask your teacher or the study office for your timetable/courses as CSV.",
        mock: ["Lectio skema 2026-2027.csv", "Lectio hold 2026-2027.csv"],
      },
      {
        icon: Upload,
        title: da ? "Upload filen" : "Upload the file",
        text: da
          ? "Vælg 'Ludus · Niels Brock' herover og tryk 'Vælg fil'. Semikolon-filer forstås automatisk."
          : "Choose 'Ludus · Niels Brock' above and press 'Choose file'. Semicolon files are understood automatically.",
        mock: ['"Aktivitet";"Hold";"Dato"', "▸ Vælg fil"],
      },
      {
        icon: CalendarDays,
        title: da ? "Tjek og gem" : "Review and save",
        text: da
          ? "Fjern flueben ved det, du ikke vil have med, og tryk 'Gem valgte'. Lektioner lander i Studieplan."
          : "Untick what you don't want and press 'Save selected'. Lessons land in the Study planner.",
        mock: ["☑ Dansk modul · 08:15", "☐ Studietur", "▸ Gem valgte"],
      },
    ],
    tip: da
      ? "Samme fil to gange? Ingen fare – dubletter springes over."
      : "Same file twice? No problem – duplicates are skipped.",
  },
});

export function ImportGuide({ lang }: { lang: Language }) {
  const da = lang === "da";
  const [active, setActive] = useState<GuideKey>("lectio");
  const guide = guides(da)[active];
  return (
    <Card className="border-slate-200/80 shadow-sm" data-testid="import-guide-card">
      <CardHeader className="gap-3">
        <CardTitle className="font-heading text-lg">
          {da ? "Sådan får du dine data ud" : "How to get your data out"}
        </CardTitle>
        <div className="flex flex-wrap gap-2" data-testid="import-guide-tabs">
          {(Object.keys(guides(da)) as GuideKey[]).map((key) => (
            <button
              key={key}
              onClick={() => setActive(key)}
              className={`rounded-full border px-3 py-1.5 text-xs font-semibold ${active === key ? "border-blue-200 bg-blue-50 text-blue-700" : "border-slate-200 text-slate-500 hover:bg-slate-50"}`}
              data-testid={`import-guide-tab-${key}`}
            >
              {guides(da)[key].name}
            </button>
          ))}
        </div>
      </CardHeader>
      <CardContent className="space-y-4">
        <ol className="grid gap-3 md:grid-cols-3">
          {guide.steps.map((step, index) => (
            <li
              key={step.title}
              className="rounded-2xl border border-slate-100 bg-white p-4"
              data-testid="import-guide-step"
            >
              <div
                className="mb-3 rounded-xl border border-blue-100 bg-gradient-to-b from-blue-50 to-white p-3"
                aria-hidden
              >
                <div className="mb-2 flex gap-1">
                  <span className="size-1.5 rounded-full bg-blue-200" />
                  <span className="size-1.5 rounded-full bg-blue-200" />
                  <span className="size-1.5 rounded-full bg-blue-200" />
                </div>
                {step.mock.map((line) => (
                  <p
                    key={line}
                    className={`truncate rounded px-1.5 py-0.5 font-mono text-[10px] ${line.startsWith("▸") ? "bg-blue-600 text-white" : "text-slate-500"}`}
                  >
                    {line}
                  </p>
                ))}
              </div>
              <p className="flex items-center gap-2 text-sm font-semibold text-slate-800">
                <span className="grid size-6 place-items-center rounded-full bg-blue-600 text-[11px] text-white">
                  {index + 1}
                </span>
                <step.icon className="size-4 text-blue-600" />
                {step.title}
              </p>
              <p className="mt-2 text-xs leading-relaxed text-slate-500">{step.text}</p>
            </li>
          ))}
        </ol>
        <p
          className="rounded-xl bg-blue-50/70 p-3 text-xs leading-relaxed text-blue-800"
          data-testid="import-guide-tip"
        >
          {guide.tip}
        </p>
      </CardContent>
    </Card>
  );
}
