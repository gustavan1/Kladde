import { useEffect, useMemo, useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import {
  ArrowRight,
  Bell,
  BookOpen,
  CalendarDays,
  Check,
  CheckCircle2,
  ChevronRight,
  Clock3,
  Download,
  FileText,
  Globe2,
  LayoutDashboard,
  ListTodo,
  Pencil,
  Pin,
  PinOff,
  Plus,
  Search,
  Sparkles,
  Target,
  Trash2,
  Upload,
  X,
} from "lucide-react";
import { CalendarLinks, ImportGuide } from "@/components/kladde/ImportExtras";
import { WeeklyCard } from "@/components/kladde/StudyExtras";
import { AiView, NoteAiTools, ProjectPlanner } from "@/components/kladde/AiExtras";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { ApiError, apiDelete, apiGet, apiPatch, apiPost, errorDetail, exportBackup } from "@/lib/api";

type Language = "da" | "en";
type View = "overview" | "notes" | "homework" | "planner" | "ai" | "imports";
type Subject = { id: string; name: string; key: string; color: string };
type Note = {
  id: string;
  title: string;
  content: string;
  subject: string;
  subject_key: string;
  updated_at: string;
  created_at: string;
  is_pinned: boolean;
};
type Task = {
  id: string;
  title: string;
  subject: string;
  subject_key: string;
  deadline: string;
  status: "todo" | "doing" | "done";
  priority: "high" | "medium" | "low";
  estimated_minutes: number;
  created_at: string;
};
type Block = {
  id: string;
  day: string;
  time: string;
  label: string;
  subject: string;
  color: string;
  source: string | null;
  minutes: number | null;
};
type Dashboard = {
  today: string;
  notes: Note[];
  tasks: Task[];
  subjects: Subject[];
  study_blocks: Block[];
  reminders: Task[];
};
type StudyPlan = { blocks: Block[]; summary: string };
// id is set when editing an existing note and missing for a new one.
type NoteDraft = { id?: string; title: string; content: string; subject: string; subject_key: string };
type TaskDraft = NoteDraft & {
  id?: string;
  deadline: string;
  priority: Task["priority"];
  estimated_minutes: number;
};
type ImportSource = "lectio" | "ludus" | "mitnielsbrock";
type ImportItem = {
  kind: "deadline" | "lesson" | "subject";
  title: string;
  subject: string;
  subject_key: string;
  date: string | null;
  day: string | null;
  time: string | null;
  end_time: string | null;
  notes: string | null;
  external_id: string | null;
  selected: boolean;
};
type ImportPreview = {
  source: ImportSource;
  source_label: string;
  detected_format: string;
  items: ImportItem[];
  warnings: string[];
};
type ImportCommit = {
  imported_tasks: number;
  imported_blocks: number;
  imported_subjects: number;
  skipped_duplicates: number;
  message: string;
};

const text = {
  da: {
    over: "Overblik",
    notes: "Noter & fag",
    tasks: "Lektier & opgaver",
    plan: "Studieplan",
    search: "Søg i noter og lektier…",
    newNote: "Ny note",
    addTask: "Tilføj opgave",
    editTask: "Redigér opgave",
    recent: "Seneste noter",
    deadlines: "Kommende deadlines",
    noteHint: "Saml dine vigtigste tanker, formler og kilder ét sted.",
    save: "Gem",
    cancel: "Annuller",
    delete: "Slet",
    noNotes: "Ingen noter matcher din søgning",
    placeholder: "Skriv dine noter her…",
    priority: { high: "Høj", medium: "Mellem", low: "Lav" },
  },
  en: {
    over: "Overview",
    notes: "Notes & subjects",
    tasks: "Homework & tasks",
    plan: "Study planner",
    search: "Search notes and homework…",
    newNote: "New note",
    addTask: "Add task",
    editTask: "Edit task",
    recent: "Recent notes",
    deadlines: "Upcoming deadlines",
    noteHint: "Keep your key ideas, formulas and sources in one calm workspace.",
    save: "Save",
    cancel: "Cancel",
    delete: "Delete",
    noNotes: "No notes match your search",
    placeholder: "Write your notes here…",
    priority: { high: "High", medium: "Medium", low: "Low" },
  },
} as const;

// Subject colours. Each subject stores a colour name; these are the matching classes.
const colorTone: Record<string, string> = {
  blue: "bg-blue-50 text-blue-700 border-blue-100",
  indigo: "bg-indigo-50 text-indigo-700 border-indigo-100",
  emerald: "bg-emerald-50 text-emerald-700 border-emerald-100",
  amber: "bg-amber-50 text-amber-700 border-amber-100",
  lime: "bg-lime-50 text-lime-700 border-lime-100",
  rose: "bg-rose-50 text-rose-700 border-rose-100",
  sky: "bg-sky-50 text-sky-700 border-sky-100",
  violet: "bg-violet-50 text-violet-700 border-violet-100",
  teal: "bg-teal-50 text-teal-700 border-teal-100",
  orange: "bg-orange-50 text-orange-700 border-orange-100",
};
const palette = Object.keys(colorTone);
const subjectColors = new Map<string, string>();
const toneFor = (key: string) => {
  const color =
    subjectColors.get(key) ?? palette[[...key].reduce((h, c) => h + c.charCodeAt(0), 0) % palette.length];
  return colorTone[color] ?? colorTone.blue;
};
const subjectKey = (name: string) => name.toLowerCase().replaceAll(" & ", "_");

const dateLabel = (value: string, lang: Language) =>
  new Intl.DateTimeFormat(lang === "da" ? "da-DK" : "en-GB", { day: "numeric", month: "short" }).format(
    new Date(`${value}T12:00:00`),
  );
const ageLabel = (value: string, lang: Language) => {
  const minutes = Math.max(0, Math.round((Date.now() - new Date(value).getTime()) / 60000));
  if (minutes < 60)
    return lang === "da"
      ? minutes < 2
        ? "lige nu"
        : `${minutes} min siden`
      : minutes < 2
        ? "just now"
        : `${minutes} min ago`;
  const hours = Math.round(minutes / 60);
  return hours < 24
    ? lang === "da"
      ? `${hours} t siden`
      : `${hours}h ago`
    : lang === "da"
      ? `${Math.round(hours / 24)} dage siden`
      : `${Math.round(hours / 24)}d ago`;
};
const greeting = (lang: Language) => {
  const h = new Date().getHours();
  if (lang === "en") return h < 12 ? "Good morning" : h < 18 ? "Good afternoon" : "Good evening";
  return h < 10 ? "Godmorgen" : h < 12 ? "God formiddag" : h < 18 ? "God eftermiddag" : "God aften";
};
const readStorage = (key: string) => {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
};
const writeStorage = (key: string, value: string) => {
  try {
    localStorage.setItem(key, value);
  } catch {
    /* private mode: just don't remember */
  }
};

function SubjectTag({ subject, subject_key }: { subject: string; subject_key: string }) {
  return (
    <span
      className={`inline-flex rounded-md border px-2 py-1 text-[11px] font-semibold ${toneFor(subject_key)}`}
      data-testid={`subject-tag-${subject_key}`}
    >
      {subject}
    </span>
  );
}

function SubjectSelect({
  subjects,
  value,
  onChange,
  className,
  testId,
}: {
  subjects: Subject[];
  value: string;
  onChange: (s: Subject) => void;
  className: string;
  testId: string;
}) {
  return (
    <select
      value={value}
      onChange={(e) => {
        const s = subjects.find((x) => x.key === e.target.value);
        if (s) onChange(s);
      }}
      className={className}
      data-testid={testId}
    >
      {subjects.map((s) => (
        <option value={s.key} key={s.key}>
          {s.name}
        </option>
      ))}
    </select>
  );
}

/** An in-app "are you sure?" bar (the browser's confirm() box is not available in every window). */
function ConfirmDelete({
  text,
  yes,
  no,
  onYes,
  onNo,
  testId,
}: {
  text: string;
  yes: string;
  no: string;
  onYes: () => void;
  onNo: () => void;
  testId: string;
}) {
  return (
    <div
      className="flex flex-wrap items-center gap-2 rounded-xl border border-red-100 bg-red-50 px-3 py-2 text-sm text-red-700"
      role="alert"
      data-testid={testId}
    >
      <span className="min-w-0 flex-1">{text}</span>
      <Button type="button" size="sm" variant="outline" className="bg-white" onClick={onNo}>
        {no}
      </Button>
      <Button
        type="button"
        size="sm"
        className="bg-red-600 text-white hover:bg-red-700"
        onClick={onYes}
        data-testid={`${testId}-yes`}
      >
        {yes}
      </Button>
    </div>
  );
}

function Sidebar({
  active,
  setActive,
  lang,
  nextTask,
}: {
  active: View;
  setActive: (v: View) => void;
  lang: Language;
  nextTask?: Task;
}) {
  const t = text[lang];
  const links: [View, string, typeof LayoutDashboard][] = [
    ["overview", t.over, LayoutDashboard],
    ["notes", t.notes, BookOpen],
    ["homework", t.tasks, ListTodo],
    ["planner", t.plan, CalendarDays],
    ["ai", lang === "da" ? "AI-hjælper" : "AI helper", Sparkles],
    ["imports", lang === "da" ? "Importér fra skole" : "School import", Upload],
  ];
  return (
    <aside
      className="sticky top-0 hidden h-svh w-64 shrink-0 flex-col border-r border-slate-200/80 bg-white px-4 py-6 lg:flex"
      data-testid="app-sidebar"
    >
      <div className="flex items-center gap-3 px-3">
        <span className="grid size-9 place-items-center rounded-xl bg-[#0b1d3a] text-white">
          <BookOpen className="size-4" />
        </span>
        <div>
          <p className="font-heading text-lg font-semibold text-[#0b1d3a]">Kladde</p>
          <p className="text-[10px] font-semibold uppercase tracking-[0.16em] text-slate-400">
            Study workspace
          </p>
        </div>
      </div>
      <nav className="mt-10 space-y-1" data-testid="primary-navigation">
        {links.map(([id, label, Icon]) => (
          <button
            key={id}
            onClick={() => setActive(id)}
            className={`flex w-full items-center gap-3 rounded-xl px-3 py-3 text-left text-sm font-medium ${active === id ? "bg-blue-50 text-blue-700" : "text-slate-500 hover:bg-slate-50 hover:text-slate-800"}`}
            data-testid={`sidebar-nav-${id}`}
          >
            <Icon className="size-4" />
            {label}
          </button>
        ))}
      </nav>
      {nextTask && (
        <div
          className="mt-auto rounded-2xl border border-blue-100 bg-blue-50/70 p-4"
          data-testid="sidebar-focus-card"
        >
          <div className="flex items-center gap-2 text-xs font-semibold text-blue-700">
            <Target className="size-4" /> {lang === "da" ? "Næste frist" : "Next deadline"}
          </div>
          <p
            className="mt-3 text-sm font-medium leading-relaxed text-slate-700"
            data-testid="sidebar-focus-title"
          >
            {nextTask.title}
          </p>
          <p className="mt-1 text-xs text-slate-500">
            {nextTask.subject} · {dateLabel(nextTask.deadline, lang)}
          </p>
          <button
            onClick={() => setActive("homework")}
            className="mt-3 text-xs font-semibold text-blue-700"
            data-testid="sidebar-focus-action"
          >
            {lang === "da" ? "Se opgave" : "View task"}
          </button>
        </div>
      )}
    </aside>
  );
}

function Header({
  lang,
  setLang,
  setActive,
  search,
  setSearch,
  reminders,
  name,
  setName,
}: {
  lang: Language;
  setLang: (v: Language) => void;
  setActive: (v: View) => void;
  search: string;
  setSearch: (v: string) => void;
  reminders: Task[];
  name: string;
  setName: (v: string) => void;
}) {
  const t = text[lang];
  const [open, setOpen] = useState<"reminders" | "profile" | null>(null);
  const [nameDraft, setNameDraft] = useState(name);
  const initials =
    name
      .split(/\s+/)
      .filter(Boolean)
      .map((p) => p[0])
      .join("")
      .slice(0, 2)
      .toUpperCase() || "K";
  const searchInput = (testId: string) => (
    <div className="relative w-full">
      <Search className="absolute left-3 top-1/2 size-4 -translate-y-1/2 text-slate-400" />
      <Input
        value={search}
        onChange={(e) => setSearch(e.target.value)}
        placeholder={t.search}
        className="h-10 rounded-xl border-slate-200 bg-slate-50 pl-10 pr-9"
        data-testid={testId}
      />
      {search && (
        <button
          onClick={() => setSearch("")}
          className="absolute right-2 top-1/2 grid size-6 -translate-y-1/2 place-items-center rounded-md text-slate-400 hover:bg-slate-200"
          title={lang === "da" ? "Ryd søgning" : "Clear search"}
        >
          <X className="size-3.5" />
        </button>
      )}
    </div>
  );
  return (
    <header
      className="sticky top-0 z-20 border-b border-slate-200/80 bg-white/90 px-4 py-3 backdrop-blur-xl sm:px-6"
      data-testid="app-header"
    >
      <div className="flex items-center gap-3">
        <div className="flex items-center gap-2 lg:hidden">
          <span className="grid size-9 place-items-center rounded-xl bg-[#0b1d3a] text-white">
            <BookOpen className="size-4" />
          </span>
          <span className="font-heading font-semibold text-[#0b1d3a]">Kladde</span>
        </div>
        <div className="hidden max-w-xl flex-1 md:block">{searchInput("omnisearch-input")}</div>
        <div className="ml-auto flex items-center gap-2">
          <div
            className="flex items-center rounded-lg border border-slate-200 bg-slate-50 p-0.5"
            data-testid="language-switcher"
          >
            <button
              onClick={() => setLang("da")}
              className={`rounded-md px-2 py-1 text-xs font-semibold ${lang === "da" ? "bg-white text-blue-700 shadow-sm" : "text-slate-400"}`}
              data-testid="language-toggle-btn"
            >
              DA
            </button>
            <button
              onClick={() => setLang("en")}
              className={`rounded-md px-2 py-1 text-xs font-semibold ${lang === "en" ? "bg-white text-blue-700 shadow-sm" : "text-slate-400"}`}
              data-testid="language-toggle-en-btn"
            >
              EN
            </button>
          </div>
          <div className="relative">
            <button
              onClick={() => setOpen(open === "reminders" ? null : "reminders")}
              className="relative grid size-9 place-items-center rounded-xl text-slate-500 hover:bg-blue-50 hover:text-blue-600"
              title={lang === "da" ? "Påmindelser" : "Reminders"}
              data-testid="reminders-bell-button"
            >
              <Bell className="size-4" />
              {reminders.length > 0 && (
                <span
                  className="absolute -right-0.5 -top-0.5 grid size-4 place-items-center rounded-full bg-red-500 text-[10px] font-bold text-white"
                  data-testid="reminders-count"
                >
                  {reminders.length}
                </span>
              )}
            </button>
            {open === "reminders" && (
              <div
                className="absolute right-0 top-11 z-30 w-72 rounded-2xl border border-slate-200 bg-white p-3 shadow-xl"
                data-testid="reminders-panel"
              >
                <p className="px-1 pb-2 text-xs font-semibold uppercase tracking-wide text-slate-400">
                  {lang === "da" ? "Afleveres i dag eller i morgen" : "Due today or tomorrow"}
                </p>
                {reminders.length === 0 ? (
                  <p className="px-1 py-3 text-sm text-slate-500" data-testid="reminders-empty">
                    {lang === "da" ? "Ingen frister lige nu 🎉" : "No deadlines right now 🎉"}
                  </p>
                ) : (
                  reminders.map((task) => (
                    <button
                      key={task.id}
                      onClick={() => {
                        setOpen(null);
                        setActive("homework");
                      }}
                      className="flex w-full items-start gap-2 rounded-xl px-2 py-2 text-left hover:bg-slate-50"
                      data-testid="reminder-item"
                    >
                      <Clock3 className="mt-0.5 size-4 shrink-0 text-red-500" />
                      <span>
                        <span className="block text-sm font-semibold text-slate-800">{task.title}</span>
                        <span className="text-xs text-slate-400">
                          {task.subject} · {dateLabel(task.deadline, lang)}
                        </span>
                      </span>
                    </button>
                  ))
                )}
              </div>
            )}
          </div>
          <button
            onClick={() => setActive("ai")}
            className="hidden size-9 place-items-center rounded-xl text-slate-400 hover:bg-blue-50 hover:text-blue-600 sm:grid"
            title={lang === "da" ? "AI-hjælper" : "AI helper"}
            data-testid="header-ai-button"
          >
            <Sparkles className="size-4" />
          </button>
          <div className="relative flex items-center gap-2 border-l border-slate-200 pl-3">
            <button
              onClick={() => {
                setNameDraft(name);
                setOpen(open === "profile" ? null : "profile");
              }}
              className="flex items-center gap-2 rounded-xl px-1 py-0.5 hover:bg-slate-50"
              title={lang === "da" ? "Skift navn" : "Change name"}
              data-testid="profile-button"
            >
              <span className="hidden text-right sm:block">
                <span className="block text-xs font-semibold text-slate-700" data-testid="user-display-name">
                  {name || (lang === "da" ? "Elev" : "Student")}
                </span>
                <span className="block text-[10px] text-slate-400">
                  {lang === "da" ? "Min studieplads" : "My workspace"}
                </span>
              </span>
              <span className="grid size-8 place-items-center rounded-full bg-blue-600 text-xs font-semibold text-white">
                {initials}
              </span>
            </button>
            {open === "profile" && (
              <form
                onSubmit={(e) => {
                  e.preventDefault();
                  setName(nameDraft.trim());
                  setOpen(null);
                }}
                className="absolute right-0 top-11 z-30 w-64 space-y-3 rounded-2xl border border-slate-200 bg-white p-4 shadow-xl"
                data-testid="profile-panel"
              >
                <p className="text-xs font-semibold uppercase tracking-wide text-slate-400">
                  {lang === "da" ? "Dit navn" : "Your name"}
                </p>
                <Input
                  autoFocus
                  value={nameDraft}
                  onChange={(e) => setNameDraft(e.target.value)}
                  placeholder={lang === "da" ? "Fx Mads" : "E.g. Sam"}
                  className="h-10 rounded-xl"
                  data-testid="profile-name-input"
                />
                <div className="flex justify-end gap-2">
                  <Button type="button" variant="outline" size="sm" onClick={() => setOpen(null)}>
                    {t.cancel}
                  </Button>
                  <Button type="submit" size="sm" className="bg-blue-600 hover:bg-blue-700">
                    {t.save}
                  </Button>
                </div>
              </form>
            )}
          </div>
        </div>
      </div>
      <div className="mt-3 flex md:hidden">{searchInput("mobile-omnisearch-input")}</div>
    </header>
  );
}

function Overview({
  data,
  lang,
  name,
  setActive,
  setSelected,
  startNewNote,
}: {
  data?: Dashboard;
  lang: Language;
  name: string;
  setActive: (v: View) => void;
  setSelected: (n: Note) => void;
  startNewNote: () => void;
}) {
  const t = text[lang];
  const open = (data?.tasks || []).filter((x) => x.status !== "done");
  const reminders = data?.reminders || [];
  const today = data?.today ?? "";
  return (
    <div className="space-y-8" data-testid="overview-view">
      {reminders.length > 0 && (
        <button
          onClick={() => setActive("homework")}
          className="flex w-full items-center gap-3 rounded-2xl border border-red-100 bg-red-50/70 p-4 text-left hover:bg-red-50"
          data-testid="reminder-banner"
        >
          <span className="grid size-9 shrink-0 place-items-center rounded-xl bg-white text-red-500">
            <Bell className="size-4" />
          </span>
          <span className="min-w-0 flex-1">
            <span className="block text-sm font-semibold text-red-700">
              {lang === "da"
                ? `${reminders.length} ${reminders.length === 1 ? "frist" : "frister"} i dag eller i morgen`
                : `${reminders.length} deadline${reminders.length === 1 ? "" : "s"} today or tomorrow`}
            </span>
            <span className="block truncate text-xs text-red-600/80">
              {reminders.map((r) => r.title).join(" · ")}
            </span>
          </span>
          <ChevronRight className="size-4 text-red-400" />
        </button>
      )}
      <div className="flex flex-col justify-between gap-4 sm:flex-row sm:items-end">
        <div>
          <p className="text-sm font-medium text-blue-600">
            {greeting(lang)}
            {name ? `, ${name}` : ""} 👋
          </p>
          <h1
            className="mt-1 font-heading text-3xl font-semibold tracking-tight text-[#0b1d3a] sm:text-4xl"
            data-testid="overview-title"
          >
            {lang === "da" ? "Klar til en god studiedag?" : "Ready for a good study day?"}
          </h1>
          <p className="mt-2 text-sm text-slate-500">
            {lang === "da" ? "Her er det vigtigste, der venter på dig." : "Here is what is waiting for you."}
          </p>
        </div>
        <Button onClick={startNewNote} className="bg-blue-600 hover:bg-blue-700" data-testid="new-note-btn">
          <Plus className="size-4" /> {t.newNote}
        </Button>
      </div>
      <WeeklyCard lang={lang} />
      <div className="grid gap-4 sm:grid-cols-3">
        <Stat
          icon={FileText}
          value={data?.notes.length || 0}
          label={lang === "da" ? "Aktive noter" : "Active notes"}
        />
        <Stat icon={ListTodo} value={open.length} label={lang === "da" ? "Åbne opgaver" : "Open tasks"} />
        <Stat
          icon={CheckCircle2}
          value={(data?.tasks || []).filter((x) => x.status === "done").length}
          label={lang === "da" ? "Færdiggjort" : "Completed"}
        />
      </div>
      <div className="grid gap-5 xl:grid-cols-[1.15fr_0.85fr]">
        <Card className="overflow-hidden border-blue-100 bg-[#0b1d3a] text-white shadow-xl shadow-blue-900/10">
          <CardContent className="relative p-7">
            <div className="flex items-center gap-2 text-sm font-medium text-blue-200">
              <Sparkles className="size-4" />{" "}
              {lang === "da" ? "Fortsæt hvor du slap" : "Continue where you left off"}
            </div>
            {data?.notes[0] ? (
              <>
                <h2 className="mt-4 font-heading text-2xl font-semibold" data-testid="continue-note-title">
                  {data.notes[0].title}
                </h2>
                <p className="mt-3 line-clamp-2 max-w-md text-sm leading-relaxed text-slate-300">
                  {data.notes[0].content.replace(/[#*]/g, "").slice(0, 140) ||
                    (lang === "da" ? "Tom note" : "Empty note")}
                </p>
                <button
                  onClick={() => {
                    if (data?.notes[0]) {
                      setSelected(data.notes[0]);
                      setActive("notes");
                    }
                  }}
                  className="mt-7 inline-flex items-center gap-2 text-sm font-semibold"
                  data-testid="continue-note-button"
                >
                  {lang === "da" ? "Åbn note" : "Open note"} <ArrowRight className="size-4" />
                </button>
              </>
            ) : (
              <>
                <h2 className="mt-4 font-heading text-2xl font-semibold" data-testid="continue-empty-title">
                  {lang === "da" ? "Skriv din første note" : "Write your first note"}
                </h2>
                <p className="mt-3 max-w-md text-sm leading-relaxed text-slate-300">
                  {lang === "da"
                    ? "Saml dine noter pr. fag, så har du altid styr på stoffet til prøver og eksamen."
                    : "Collect notes by subject, so you always have the material ready for tests and exams."}
                </p>
                <button
                  onClick={startNewNote}
                  className="mt-7 inline-flex items-center gap-2 text-sm font-semibold"
                  data-testid="continue-empty-button"
                >
                  {lang === "da" ? "Skriv en note" : "Write a note"} <ArrowRight className="size-4" />
                </button>
              </>
            )}
          </CardContent>
        </Card>
        <Card className="border-slate-200/80">
          <CardHeader className="flex flex-row items-center justify-between border-b border-slate-100 pb-4">
            <CardTitle className="font-heading text-lg">{t.deadlines}</CardTitle>
            <button
              onClick={() => setActive("homework")}
              className="text-xs font-semibold text-blue-600"
              data-testid="view-all-deadlines-button"
            >
              {lang === "da" ? "Se alle" : "View all"}
            </button>
          </CardHeader>
          <CardContent className="p-0">
            {open.slice(0, 3).map((task) => {
              const overdue = task.deadline < today;
              return (
                <button
                  onClick={() => setActive("homework")}
                  className="flex w-full items-center gap-3 border-b border-slate-100 px-5 py-4 text-left last:border-0 hover:bg-slate-50/70"
                  key={task.id}
                  data-testid="homework-task-card"
                >
                  <span
                    className={`grid size-9 place-items-center rounded-xl ${overdue ? "bg-red-50 text-red-500" : "bg-amber-50 text-amber-600"}`}
                  >
                    <Clock3 className="size-4" />
                  </span>
                  <div className="min-w-0 flex-1">
                    <p className="truncate text-sm font-semibold text-slate-800">{task.title}</p>
                    <p className={`mt-1 text-xs ${overdue ? "text-red-500" : "text-slate-400"}`}>
                      {task.subject} · {dateLabel(task.deadline, lang)}
                      {overdue ? (lang === "da" ? " · overskredet" : " · overdue") : ""}
                    </p>
                  </div>
                  <ChevronRight className="size-4 text-slate-300" />
                </button>
              );
            })}
            {open.length === 0 && (
              <div className="p-6 text-sm text-slate-500" data-testid="deadlines-empty">
                {lang === "da"
                  ? "Ingen åbne opgaver. Tilføj en eller importér fra skolen."
                  : "No open tasks. Add one or import from school."}
              </div>
            )}
          </CardContent>
        </Card>
      </div>
      <div>
        <div className="mb-4 flex items-center justify-between">
          <h2 className="font-heading text-xl font-semibold text-[#0b1d3a]">{t.recent}</h2>
          <button
            onClick={() => setActive("notes")}
            className="text-sm font-semibold text-blue-600"
            data-testid="view-all-notes-button"
          >
            {lang === "da" ? "Se noter" : "View notes"}
          </button>
        </div>
        <div className="grid gap-4 md:grid-cols-3">
          {(data?.notes || []).slice(0, 3).map((note) => (
            <button
              onClick={() => {
                setSelected(note);
                setActive("notes");
              }}
              className="rounded-2xl border border-slate-200 bg-white p-5 text-left shadow-sm hover:-translate-y-0.5 hover:border-blue-200"
              key={note.id}
              data-testid="note-card-item"
            >
              <div className="flex items-center justify-between">
                <SubjectTag subject={note.subject} subject_key={note.subject_key} />
                {note.is_pinned && <Pin className="size-3.5 text-blue-500" />}
              </div>
              <h3 className="mt-4 line-clamp-2 font-heading text-lg font-semibold text-slate-800">
                {note.title}
              </h3>
              <p className="mt-2 line-clamp-2 text-sm text-slate-500">
                {note.content.replace(/[#*]/g, "").slice(0, 95)}
              </p>
              <p className="mt-5 text-xs text-slate-400">{ageLabel(note.updated_at, lang)}</p>
            </button>
          ))}
          {!data?.notes.length && (
            <p className="text-sm text-slate-500" data-testid="recent-notes-empty">
              {lang === "da" ? "Dine seneste noter dukker op her." : "Your latest notes will show up here."}
            </p>
          )}
        </div>
      </div>
    </div>
  );
}

function Stat({ icon: Icon, value, label }: { icon: typeof FileText; value: number; label: string }) {
  return (
    <Card className="border-slate-200/80 shadow-sm">
      <CardContent className="flex items-center gap-4 p-5">
        <span className="grid size-10 place-items-center rounded-xl bg-blue-50 text-blue-600">
          <Icon className="size-5" />
        </span>
        <div>
          <p className="font-heading text-2xl font-semibold text-[#0b1d3a]">{value}</p>
          <p className="text-xs font-medium text-slate-500">{label}</p>
        </div>
      </CardContent>
    </Card>
  );
}

function Notes({
  data,
  lang,
  search,
  selected,
  setSelected,
  refresh,
  startNew,
  onStarted,
  openAi,
}: {
  data?: Dashboard;
  lang: Language;
  search: string;
  selected: Note | null;
  setSelected: (n: Note | null) => void;
  refresh: () => void;
  startNew: boolean;
  onStarted: () => void;
  openAi: () => void;
}) {
  const t = text[lang];
  const da = lang === "da";
  const [filter, setFilter] = useState("all");
  const [draft, setDraft] = useState<NoteDraft | null>(null);
  const [newSubject, setNewSubject] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);
  const subjects = data?.subjects || [];
  const save = useMutation({
    mutationFn: (payload: NoteDraft & { id?: string }) =>
      payload.id ? apiPatch<Note>(`/notes/${payload.id}`, payload) : apiPost<Note>("/notes", payload),
    onSuccess: (note) => {
      setSelected(note);
      setDraft(null);
      refresh();
      toast.success(da ? "Note gemt" : "Note saved");
    },
    onError: (e) =>
      toast.error(errorDetail(e, da ? "Noten kunne ikke gemmes" : "The note could not be saved")),
  });
  const pin = useMutation({
    mutationFn: (note: Note) => apiPatch<Note>(`/notes/${note.id}`, { is_pinned: !note.is_pinned }),
    onSuccess: (note) => {
      setSelected(note);
      refresh();
    },
  });
  const remove = useMutation({
    mutationFn: (note: Note) => apiDelete(`/notes/${note.id}`),
    onSuccess: () => {
      setSelected(null);
      setDraft(null);
      refresh();
      toast.success(da ? "Note slettet" : "Note deleted");
    },
  });
  const addSubject = useMutation({
    mutationFn: (name: string) => apiPost<Subject>("/subjects", { name }),
    onSuccess: (s) => {
      setNewSubject(null);
      setFilter(s.key);
      refresh();
    },
    onError: (e) =>
      toast.error(errorDetail(e, da ? "Faget kunne ikke tilføjes" : "Subject could not be added")),
  });
  const q = search.toLowerCase();
  const notes = (data?.notes || []).filter(
    (n) =>
      (filter === "all" || n.subject_key === filter) &&
      `${n.title} ${n.content} ${n.subject}`.toLowerCase().includes(q),
  );
  const newNote = () => {
    const s = subjects.find((x) => x.key === filter) ?? subjects[0];
    setSelected(null);
    setDraft({ title: "", content: "", subject: s?.name ?? "Dansk", subject_key: s?.key ?? "dansk" });
  };
  useEffect(() => {
    if (startNew) {
      newNote();
      onStarted();
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [startNew]);
  const submit = () => {
    if (draft && draft.title.trim() && !save.isPending) save.mutate(draft);
  };
  return (
    <div className="space-y-6" data-testid="notes-view">
      <div className="flex flex-col justify-between gap-4 sm:flex-row sm:items-end">
        <div>
          <p className="text-sm font-medium text-blue-600">
            {da ? "Dit faglige bibliotek" : "Your academic library"}
          </p>
          <h1 className="mt-1 font-heading text-3xl font-semibold tracking-tight text-[#0b1d3a]">
            {t.notes}
          </h1>
          <p className="mt-2 text-sm text-slate-500">{t.noteHint}</p>
        </div>
        <Button onClick={newNote} className="bg-blue-600 hover:bg-blue-700" data-testid="notes-new-button">
          <Plus className="size-4" /> {t.newNote}
        </Button>
      </div>
      <div className="grid gap-5 xl:grid-cols-[190px_300px_minmax(0,1fr)]">
        <Card className="h-fit">
          <CardHeader>
            <CardTitle className="font-heading text-sm">{da ? "Fag" : "Subjects"}</CardTitle>
          </CardHeader>
          <CardContent className="space-y-1 px-3 pb-4">
            {[{ key: "all", name: da ? "Alle noter" : "All notes" }, ...subjects].map((s) => (
              <button
                key={s.key}
                onClick={() => setFilter(s.key)}
                className={`flex w-full justify-between rounded-lg px-3 py-2 text-left text-xs font-medium ${filter === s.key ? "bg-blue-50 text-blue-700" : "text-slate-500 hover:bg-slate-50"}`}
                data-testid={s.key === "all" ? "subject-folder-all" : "subject-folder-item"}
              >
                {s.name}
                <span className="text-slate-400">
                  {s.key === "all"
                    ? (data?.notes || []).length
                    : data?.notes.filter((n) => n.subject_key === s.key).length}
                </span>
              </button>
            ))}
            {newSubject === null ? (
              <button
                onClick={() => setNewSubject("")}
                className="mt-2 flex w-full items-center gap-1.5 rounded-lg px-3 py-2 text-left text-xs font-semibold text-blue-600 hover:bg-blue-50"
                data-testid="subject-add-button"
              >
                <Plus className="size-3.5" /> {da ? "Nyt fag" : "New subject"}
              </button>
            ) : (
              <form
                className="mt-2 space-y-2 px-1"
                onSubmit={(e) => {
                  e.preventDefault();
                  if (newSubject.trim()) addSubject.mutate(newSubject.trim());
                }}
              >
                <Input
                  autoFocus
                  value={newSubject}
                  onChange={(e) => setNewSubject(e.target.value)}
                  onKeyDown={(e) => e.key === "Escape" && setNewSubject(null)}
                  placeholder={da ? "Fx Fysik" : "E.g. Physics"}
                  className="h-8 rounded-lg text-xs"
                  data-testid="subject-add-input"
                />
                <div className="flex gap-1">
                  <Button
                    type="submit"
                    size="sm"
                    className="h-7 flex-1 bg-blue-600 text-xs hover:bg-blue-700"
                  >
                    {da ? "Tilføj" : "Add"}
                  </Button>
                  <Button
                    type="button"
                    size="sm"
                    variant="ghost"
                    className="h-7 text-xs"
                    onClick={() => setNewSubject(null)}
                  >
                    <X className="size-3.5" />
                  </Button>
                </div>
              </form>
            )}
          </CardContent>
        </Card>
        <div className="space-y-3" data-testid="note-list">
          {notes.map((note) => (
            <button
              key={note.id}
              onClick={() => {
                setSelected(note);
                setDraft(null);
              }}
              className={`w-full rounded-2xl border p-4 text-left ${selected?.id === note.id ? "border-blue-300 bg-blue-50/40" : "border-slate-200 bg-white hover:border-blue-200"}`}
              data-testid="note-card-item"
            >
              <div className="flex items-center justify-between gap-2">
                <SubjectTag subject={note.subject} subject_key={note.subject_key} />
                <span className="flex items-center gap-1.5 text-[10px] text-slate-400">
                  {note.is_pinned && <Pin className="size-3 text-blue-500" />}
                  {ageLabel(note.updated_at, lang)}
                </span>
              </div>
              <p className="mt-3 line-clamp-1 font-heading font-semibold text-slate-800">{note.title}</p>
              <p className="mt-1 line-clamp-2 text-xs text-slate-500">{note.content.replace(/[#*]/g, "")}</p>
            </button>
          ))}
          {!notes.length && (
            <div
              className="rounded-2xl border border-dashed border-slate-200 p-6 text-center text-sm text-slate-500"
              data-testid="notes-empty-state"
            >
              {search
                ? t.noNotes
                : filter !== "all"
                  ? da
                    ? `Ingen noter i ${subjects.find((s) => s.key === filter)?.name ?? "faget"} endnu.`
                    : `No notes in ${subjects.find((s) => s.key === filter)?.name ?? "this subject"} yet.`
                  : da
                    ? "Ingen noter endnu."
                    : "No notes yet."}
            </div>
          )}
        </div>
        <Card className="min-h-[430px]" data-testid="note-editor-panel">
          {draft ? (
            <>
              <CardHeader className="flex flex-row items-center justify-between">
                <CardTitle className="font-heading">
                  {draft.id ? (da ? "Redigér note" : "Edit note") : t.newNote}
                </CardTitle>
                <button
                  onClick={() => setDraft(null)}
                  className="grid size-8 place-items-center rounded-lg text-slate-400 hover:bg-slate-100"
                  data-testid="note-editor-close-button"
                >
                  <X className="size-4" />
                </button>
              </CardHeader>
              <CardContent
                className="space-y-4"
                onKeyDown={(e) => {
                  if (e.key === "s" && (e.metaKey || e.ctrlKey)) {
                    e.preventDefault();
                    submit();
                  }
                }}
              >
                <Input
                  autoFocus={!draft.id}
                  value={draft.title}
                  onChange={(e) => setDraft({ ...draft, title: e.target.value })}
                  placeholder={da ? "Titel på note" : "Note title"}
                  className="h-11 rounded-xl font-heading text-lg"
                  data-testid="note-editor-title-input"
                />
                <SubjectSelect
                  subjects={subjects}
                  value={draft.subject_key}
                  onChange={(s) => setDraft({ ...draft, subject_key: s.key, subject: s.name })}
                  className="h-10 w-full rounded-xl border border-slate-200 bg-white px-3 text-sm"
                  testId="note-editor-subject-select"
                />
                <Textarea
                  value={draft.content}
                  onChange={(e) => setDraft({ ...draft, content: e.target.value })}
                  placeholder={t.placeholder}
                  className="min-h-[320px] rounded-xl leading-relaxed"
                  data-testid="note-editor-content-textarea"
                />
                <NoteAiTools
                  lang={lang}
                  mode="edit"
                  title={draft.title}
                  content={draft.content}
                  subject={draft.subject}
                  onSetup={openAi}
                  onUse={(text, how) =>
                    setDraft({
                      ...draft,
                      content: how === "replace" || !draft.content.trim() ? text : `${draft.content.trimEnd()}\n\n${text}`,
                    })
                  }
                />
                <div className="flex flex-wrap items-center justify-between gap-2 border-t border-slate-100 pt-4">
                  <span className="text-xs text-slate-400">
                    {draft.content.length} {da ? "tegn" : "characters"} · Ctrl+S {da ? "gemmer" : "saves"}
                  </span>
                  <div className="flex gap-2">
                    <Button
                      variant="outline"
                      onClick={() => setDraft(null)}
                      data-testid="note-editor-cancel-button"
                    >
                      {t.cancel}
                    </Button>
                    <Button
                      disabled={!draft.title.trim() || save.isPending}
                      onClick={submit}
                      className="bg-blue-600 hover:bg-blue-700"
                      data-testid="note-editor-save-btn"
                    >
                      {t.save}
                    </Button>
                  </div>
                </div>
              </CardContent>
            </>
          ) : selected ? (
            <>
              <CardHeader className="flex flex-row items-start justify-between gap-3">
                <div className="min-w-0">
                  <SubjectTag subject={selected.subject} subject_key={selected.subject_key} />
                  <CardTitle className="mt-3 font-heading text-2xl" data-testid="selected-note-title">
                    {selected.title}
                  </CardTitle>
                </div>
                <div className="flex shrink-0 gap-1">
                  <Button
                    variant="ghost"
                    size="icon"
                    onClick={() => pin.mutate(selected)}
                    title={
                      selected.is_pinned
                        ? da
                          ? "Fjern fastgørelse"
                          : "Unpin"
                        : da
                          ? "Fastgør øverst"
                          : "Pin to top"
                    }
                    data-testid="note-pin-button"
                  >
                    {selected.is_pinned ? (
                      <PinOff className="size-4 text-blue-600" />
                    ) : (
                      <Pin className="size-4 text-slate-400" />
                    )}
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon"
                    onClick={() => setConfirmDelete(selected.id)}
                    title={t.delete}
                    data-testid="note-delete-button"
                  >
                    <Trash2 className="size-4 text-slate-400" />
                  </Button>
                  <Button
                    variant="outline"
                    onClick={() =>
                      setDraft({
                        id: selected.id,
                        title: selected.title,
                        content: selected.content,
                        subject: selected.subject,
                        subject_key: selected.subject_key,
                      })
                    }
                    data-testid="note-edit-button"
                  >
                    <Pencil className="size-3.5" /> {da ? "Redigér" : "Edit"}
                  </Button>
                </div>
              </CardHeader>
              <CardContent>
                {confirmDelete === selected.id && (
                  <div className="mb-4">
                    <ConfirmDelete
                      text={da ? `Slet noten "${selected.title}"?` : `Delete the note "${selected.title}"?`}
                      yes={t.delete}
                      no={t.cancel}
                      onYes={() => {
                        setConfirmDelete(null);
                        remove.mutate(selected);
                      }}
                      onNo={() => setConfirmDelete(null)}
                      testId="note-delete-confirm"
                    />
                  </div>
                )}
                <div
                  className="whitespace-pre-wrap text-sm leading-7 text-slate-600"
                  data-testid="selected-note-content"
                >
                  {selected.content || (
                    <span className="text-slate-400">{da ? "Noten er tom." : "This note is empty."}</span>
                  )}
                </div>
                <div className="mt-6">
                  <NoteAiTools
                    key={selected.id}
                    lang={lang}
                    mode="view"
                    title={selected.title}
                    content={selected.content}
                    subject={selected.subject}
                    onSetup={openAi}
                    onUse={(text, how) =>
                      how === "new"
                        ? save.mutate({
                            title: `${selected.title} – AI`,
                            content: text,
                            subject: selected.subject,
                            subject_key: selected.subject_key,
                          })
                        : save.mutate({
                            id: selected.id,
                            title: selected.title,
                            content: `${selected.content.trimEnd()}\n\n${text}`,
                            subject: selected.subject,
                            subject_key: selected.subject_key,
                          })
                    }
                  />
                </div>
                <p className="mt-8 border-t border-slate-100 pt-4 text-xs text-slate-400">
                  {da ? "Ændret" : "Edited"} {ageLabel(selected.updated_at, lang)}
                </p>
              </CardContent>
            </>
          ) : (
            <CardContent className="flex min-h-[430px] flex-col items-center justify-center text-center">
              <FileText className="size-8 text-blue-500" />
              <h3 className="mt-4 font-heading text-xl font-semibold">
                {da ? "Vælg eller skriv en note" : "Pick or write a note"}
              </h3>
              <p className="mt-2 text-sm text-slate-500">{t.noteHint}</p>
              <Button
                onClick={newNote}
                className="mt-5 bg-blue-600 hover:bg-blue-700"
                data-testid="empty-notes-create-button"
              >
                <Plus className="size-4" /> {t.newNote}
              </Button>
            </CardContent>
          )}
        </Card>
      </div>
    </div>
  );
}

function Homework({
  data,
  lang,
  search,
  refresh,
  openAi,
}: {
  data?: Dashboard;
  lang: Language;
  search: string;
  refresh: () => void;
  openAi: () => void;
}) {
  const t = text[lang];
  const da = lang === "da";
  const [draft, setDraft] = useState<TaskDraft | null>(null);
  const [projectPlanner, setProjectPlanner] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [showDone, setShowDone] = useState(() => readStorage("kladde-show-done") !== "false");
  const formRef = useRef<HTMLDivElement>(null);
  const subjects = data?.subjects || [];
  const today = data?.today || new Date().toISOString().slice(0, 10);
  const update = useMutation({
    mutationFn: (p: { id: string; status: Task["status"] }) =>
      apiPatch<Task>(`/tasks/${p.id}`, { status: p.status }),
    onSuccess: (task) => {
      refresh();
      if (task.status === "done") toast.success(da ? "Godt gået! 🎉" : "Well done! 🎉");
    },
  });
  const save = useMutation({
    mutationFn: (p: TaskDraft) => (p.id ? apiPatch<Task>(`/tasks/${p.id}`, p) : apiPost<Task>("/tasks", p)),
    onSuccess: (_task, p) => {
      setDraft(null);
      refresh();
      toast.success(p.id ? (da ? "Opgave gemt" : "Task saved") : da ? "Opgave tilføjet" : "Task added");
    },
    onError: (e) =>
      toast.error(errorDetail(e, da ? "Opgaven kunne ikke gemmes" : "The task could not be saved")),
  });
  const remove = useMutation({
    mutationFn: (taskId: string) => apiDelete(`/tasks/${taskId}`),
    onSuccess: () => {
      setDraft(null);
      refresh();
      toast.success(da ? "Opgave slettet" : "Task deleted");
    },
  });
  const all = data?.tasks || [];
  const open = all.filter((x) => x.status !== "done");
  const q = search.toLowerCase();
  const tasks = all.filter(
    (x) => (showDone || x.status !== "done") && `${x.title} ${x.subject}`.toLowerCase().includes(q),
  );
  const doneCount = all.length - open.length;
  const openForm = (d: TaskDraft) => {
    setDraft(d);
    setConfirmDelete(false);
    setTimeout(() => formRef.current?.scrollIntoView({ behavior: "smooth", block: "nearest" }), 0);
  };
  const start = () =>
    openForm({
      title: "",
      content: "",
      subject: subjects[0]?.name ?? "Dansk",
      subject_key: subjects[0]?.key ?? "dansk",
      deadline: today,
      priority: "medium",
      estimated_minutes: 45,
    });
  const edit = (task: Task) =>
    openForm({
      id: task.id,
      title: task.title,
      content: "",
      subject: task.subject,
      subject_key: task.subject_key,
      deadline: task.deadline,
      priority: task.priority,
      estimated_minutes: task.estimated_minutes,
    });
  const priorityClass = { high: "text-red-600", medium: "text-amber-600", low: "text-slate-400" };
  return (
    <div className="space-y-6" data-testid="homework-view">
      <div className="flex flex-col justify-between gap-4 sm:flex-row sm:items-end">
        <div>
          <p className="text-sm font-medium text-blue-600">
            {da ? "Hold styr på det hele" : "Keep everything moving"}
          </p>
          <h1 className="mt-1 font-heading text-3xl font-semibold tracking-tight text-[#0b1d3a]">
            {t.tasks}
          </h1>
          <p className="mt-2 text-sm text-slate-500">
            {da
              ? "Se hvad der haster, og hvad du kan tage i morgen."
              : "See what matters today and what can wait."}
          </p>
        </div>
        <div className="flex flex-wrap gap-2">
          <Button
            variant="outline"
            onClick={() => {
              setDraft(null);
              setProjectPlanner(true);
            }}
            data-testid="project-planner-button"
          >
            <Sparkles className="size-4 text-blue-600" /> {da ? "Planlæg projekt med AI" : "Plan project with AI"}
          </Button>
          <Button onClick={start} className="bg-blue-600 hover:bg-blue-700" data-testid="new-task-btn">
            <Plus className="size-4" /> {t.addTask}
          </Button>
        </div>
      </div>
      {projectPlanner && (
        <ProjectPlanner
          lang={lang}
          subjects={subjects}
          today={today}
          onClose={() => setProjectPlanner(false)}
          onCreated={refresh}
          onSetup={openAi}
        />
      )}
      {draft && (
        <div ref={formRef}>
          <Card className="border-blue-200 shadow-lg">
            <CardHeader className="flex flex-row items-center justify-between">
              <CardTitle className="font-heading">{draft.id ? t.editTask : t.addTask}</CardTitle>
              <button
                onClick={() => setDraft(null)}
                className="grid size-8 place-items-center rounded-lg text-slate-400 hover:bg-slate-100"
              >
                <X className="size-4" />
              </button>
            </CardHeader>
            <CardContent>
              <form
                className="grid gap-4 sm:grid-cols-2"
                onSubmit={(e) => {
                  e.preventDefault();
                  if (draft.title.trim() && !save.isPending) save.mutate(draft);
                }}
              >
                <Input
                  autoFocus
                  value={draft.title}
                  onChange={(e) => setDraft({ ...draft, title: e.target.value })}
                  placeholder={da ? "Hvad skal du lave?" : "What do you need to do?"}
                  className="h-10 rounded-xl sm:col-span-2"
                  data-testid="task-title-input"
                />
                <label className="space-y-1 text-xs font-medium text-slate-500">
                  {da ? "Fag" : "Subject"}
                  <SubjectSelect
                    subjects={subjects}
                    value={draft.subject_key}
                    onChange={(s) => setDraft({ ...draft, subject_key: s.key, subject: s.name })}
                    className="h-10 w-full rounded-xl border border-slate-200 bg-white px-3 text-sm text-slate-900"
                    testId="task-subject-select"
                  />
                </label>
                <label className="space-y-1 text-xs font-medium text-slate-500">
                  {da ? "Afleveres" : "Due"}
                  <Input
                    type="date"
                    value={draft.deadline}
                    onChange={(e) => setDraft({ ...draft, deadline: e.target.value })}
                    className="h-10 rounded-xl text-slate-900"
                    data-testid="task-deadline-input"
                  />
                </label>
                <label className="space-y-1 text-xs font-medium text-slate-500">
                  {da ? "Prioritet" : "Priority"}
                  <select
                    value={draft.priority}
                    onChange={(e) => setDraft({ ...draft, priority: e.target.value as Task["priority"] })}
                    className="h-10 w-full rounded-xl border border-slate-200 bg-white px-3 text-sm text-slate-900"
                    data-testid="task-priority-select"
                  >
                    <option value="high">{t.priority.high}</option>
                    <option value="medium">{t.priority.medium}</option>
                    <option value="low">{t.priority.low}</option>
                  </select>
                </label>
                <label className="space-y-1 text-xs font-medium text-slate-500">
                  {da ? "Forventet tid (minutter)" : "Expected time (minutes)"}
                  <Input
                    type="number"
                    min={5}
                    step={5}
                    value={draft.estimated_minutes}
                    onChange={(e) => setDraft({ ...draft, estimated_minutes: Number(e.target.value) })}
                    className="h-10 rounded-xl text-slate-900"
                    data-testid="task-estimate-input"
                  />
                </label>
                <div className="flex flex-wrap justify-between gap-2 sm:col-span-2">
                  <div className="min-w-0 flex-1">
                    {draft.id &&
                      (confirmDelete ? (
                        <ConfirmDelete
                          text={da ? `Slet "${draft.title}"?` : `Delete "${draft.title}"?`}
                          yes={t.delete}
                          no={t.cancel}
                          onYes={() => remove.mutate(draft.id!)}
                          onNo={() => setConfirmDelete(false)}
                          testId="task-delete-confirm"
                        />
                      ) : (
                        <Button
                          type="button"
                          variant="ghost"
                          className="text-red-600 hover:bg-red-50 hover:text-red-700"
                          onClick={() => setConfirmDelete(true)}
                          data-testid="task-delete-button"
                        >
                          <Trash2 className="size-4" /> {t.delete}
                        </Button>
                      ))}
                  </div>
                  <div className="flex gap-2">
                    <Button type="button" variant="outline" onClick={() => setDraft(null)}>
                      {t.cancel}
                    </Button>
                    <Button
                      type="submit"
                      disabled={!draft.title.trim() || save.isPending}
                      className="bg-blue-600 hover:bg-blue-700"
                      data-testid="task-save-button"
                    >
                      {t.save}
                    </Button>
                  </div>
                </div>
              </form>
            </CardContent>
          </Card>
        </div>
      )}
      <div className="grid gap-5 lg:grid-cols-[1fr_320px]">
        <div className="space-y-3" data-testid="task-list">
          {tasks.map((task) => {
            const done = task.status === "done";
            const overdue = !done && task.deadline < today;
            return (
              <div
                className={`flex items-start gap-4 rounded-2xl border bg-white p-4 ${done ? "border-emerald-100 bg-emerald-50/30" : overdue ? "border-red-100" : "border-slate-200"}`}
                key={task.id}
                data-testid="homework-task-card"
              >
                <button
                  onClick={() => update.mutate({ id: task.id, status: done ? "todo" : "done" })}
                  className={`mt-0.5 grid size-6 shrink-0 place-items-center rounded-full border ${done ? "border-emerald-500 bg-emerald-500 text-white" : "border-slate-300 text-transparent hover:border-blue-500 hover:text-blue-300"}`}
                  title={
                    done
                      ? da
                        ? "Markér som ikke færdig"
                        : "Mark as not done"
                      : da
                        ? "Markér som færdig"
                        : "Mark as complete"
                  }
                  data-testid="homework-status-checkbox"
                >
                  <Check className="size-3.5" />
                </button>
                <button className="min-w-0 flex-1 text-left" onClick={() => edit(task)} title={t.editTask}>
                  <div className="flex flex-wrap items-center gap-2">
                    <SubjectTag subject={task.subject} subject_key={task.subject_key} />
                    <span className={`text-[11px] font-semibold ${priorityClass[task.priority]}`}>
                      {t.priority[task.priority]}
                    </span>
                  </div>
                  <p
                    className={`mt-2 font-heading font-semibold ${done ? "text-slate-400 line-through" : "text-slate-800"}`}
                    data-testid="homework-task-title"
                  >
                    {task.title}
                  </p>
                  <div className="mt-2 flex items-center gap-3 text-xs text-slate-400">
                    <span
                      className={`flex items-center gap-1 ${overdue ? "font-semibold text-red-500" : ""}`}
                    >
                      <CalendarDays className="size-3.5" /> {dateLabel(task.deadline, lang)}
                      {overdue
                        ? da
                          ? " · overskredet"
                          : " · overdue"
                        : task.deadline === today && !done
                          ? da
                            ? " · i dag"
                            : " · today"
                          : ""}
                    </span>
                    <span className="flex items-center gap-1">
                      <Clock3 className="size-3.5" /> {task.estimated_minutes} min
                    </span>
                  </div>
                </button>
                <Pencil className="mt-1 size-3.5 shrink-0 text-slate-300" />
              </div>
            );
          })}
          {!tasks.length && (
            <div className="rounded-2xl border border-dashed border-slate-200 p-10 text-center text-sm text-slate-500">
              {search
                ? da
                  ? "Ingen opgaver matcher din søgning."
                  : "No tasks match your search."
                : all.length
                  ? da
                    ? "Du er helt med. 🎉"
                    : "You are all caught up. 🎉"
                  : da
                    ? "Ingen opgaver endnu. Tilføj en, eller importér fra skolen."
                    : "No tasks yet. Add one, or import from school."}
            </div>
          )}
          {doneCount > 0 && (
            <button
              onClick={() => {
                setShowDone(!showDone);
                writeStorage("kladde-show-done", String(!showDone));
              }}
              className="text-xs font-semibold text-blue-600 hover:text-blue-800"
              data-testid="toggle-done-button"
            >
              {showDone
                ? da
                  ? `Skjul færdige (${doneCount})`
                  : `Hide completed (${doneCount})`
                : da
                  ? `Vis færdige (${doneCount})`
                  : `Show completed (${doneCount})`}
            </button>
          )}
        </div>
        <Card className="h-fit border-blue-100 bg-blue-50/60">
          <CardContent className="p-5">
            <div className="flex items-center gap-2 text-sm font-semibold text-blue-700">
              <Target className="size-4" /> {da ? "Din rytme" : "Your rhythm"}
            </div>
            <p className="mt-3 font-heading text-3xl font-semibold text-[#0b1d3a]">
              {doneCount}/{all.length}
            </p>
            <p className="mt-1 text-sm text-slate-600">
              {da ? "opgaver færdige. Små skridt tæller." : "tasks complete. Small steps count."}
            </p>
            <div className="mt-5 h-2 overflow-hidden rounded-full bg-white">
              <div
                className="h-full rounded-full bg-blue-600 transition-all duration-500"
                style={{ width: `${all.length ? (doneCount / all.length) * 100 : 0}%` }}
              />
            </div>
          </CardContent>
        </Card>
      </div>
    </div>
  );
}

function ImportCenter({ lang, refresh }: { lang: Language; refresh: () => void }) {
  const da = lang === "da";
  const [source, setSource] = useState<ImportSource>("lectio");
  const [content, setContent] = useState("");
  const [items, setItems] = useState<ImportItem[]>([]);
  const preview = useMutation({
    mutationFn: (payload: { source: ImportSource; content: string }) =>
      apiPost<ImportPreview>("/imports/preview", payload),
    onSuccess: (result) => {
      setItems(result.items);
      toast.success(da ? `${result.items.length} elementer fundet` : `${result.items.length} items found`);
    },
    onError: (e) =>
      toast.error(errorDetail(e, da ? "Teksten kunne ikke læses" : "The text could not be read")),
  });
  const commit = useMutation({
    mutationFn: (payload: { source: ImportSource; items: ImportItem[] }) =>
      apiPost<ImportCommit>("/imports/commit", payload),
    onSuccess: (result) => {
      setItems([]);
      setContent("");
      refresh();
      toast.success(result.message);
    },
    onError: (e) => toast.error(errorDetail(e, da ? "Importen fejlede" : "The import failed")),
  });
  const examples: Record<ImportSource, string> = {
    lectio:
      "Opgave,Fag,Afleveringsfrist\nDansk aflevering,Dansk,2026-09-14\nMatematik aflevering,Matematik,2026-09-18",
    ludus:
      '"Aktivitet";"Hold";"Dato";"Start";"Slut"\n"Dansk modul";"Dansk A";"2026-09-14";"08:15";"09:45"\n"Matematik modul";"Matematik B";"2026-09-15";"10:00";"11:30"',
    mitnielsbrock:
      '[{"title":"Aflevering: Marketing case","subject":"Afsætning","date":"2026-09-20"},{"title":"Afsætning modul","subject":"Afsætning","date":"2026-09-16","time":"08:15"}]',
  };
  const sourceName =
    source === "lectio" ? "Lectio" : source === "ludus" ? "Ludus · Niels Brock" : "Mit Niels Brock";
  const loadFile = async (file: File) => setContent(await file.text());
  const selectedCount = items.filter((item) => item.selected).length;
  return (
    <div className="space-y-6" data-testid="imports-view">
      <div>
        <p className="text-sm font-medium text-blue-600">
          {da ? "Flyt dit skoleoverblik til Kladde" : "Bring your school data into Kladde"}
        </p>
        <h1
          className="mt-1 font-heading text-3xl font-semibold tracking-tight text-[#0b1d3a]"
          data-testid="imports-page-title"
        >
          {da ? "Importér fra skole" : "Import from school"}
        </h1>
        <p
          className="mt-2 max-w-2xl text-sm leading-relaxed text-slate-500"
          data-testid="imports-page-description"
        >
          {da
            ? "Upload eller indsæt en eksport fra Lectio, Ludus eller Mit Niels Brock. Du ser altid en forhåndsvisning, før noget gemmes."
            : "Upload or paste an export from Lectio, Ludus or Mit Niels Brock. You always preview records before saving."}
        </p>
      </div>
      <div className="grid gap-5 xl:grid-cols-[360px_minmax(0,1fr)]">
        <Card className="h-fit border-slate-200/80 shadow-sm" data-testid="import-source-card">
          <CardHeader>
            <CardTitle className="font-heading text-lg">{da ? "Vælg kilde" : "Choose source"}</CardTitle>
          </CardHeader>
          <CardContent className="space-y-4">
            <select
              value={source}
              onChange={(e) => {
                setSource(e.target.value as ImportSource);
                setItems([]);
              }}
              className="h-11 w-full rounded-xl border border-slate-200 bg-white px-3 text-sm"
              data-testid="import-source-select"
            >
              <option value="lectio">Lectio</option>
              <option value="ludus">Ludus · Niels Brock</option>
              <option value="mitnielsbrock">Mit Niels Brock</option>
            </select>
            <div
              className="rounded-xl border border-blue-100 bg-blue-50/70 p-4"
              data-testid="import-source-guidance"
            >
              <p className="text-sm font-semibold text-blue-800">{sourceName}</p>
              <p className="mt-2 text-xs leading-relaxed text-slate-600">
                {source === "ludus"
                  ? da
                    ? "Ludus-udtræk bruger ofte semikolon-separerede CSV-filer."
                    : "Ludus exports commonly use semicolon-separated CSV files."
                  : source === "mitnielsbrock"
                    ? da
                      ? "Brug en eksport eller kopi fra din personlige Mit Niels Brock-side."
                      : "Use an export or copy from your personal Mit Niels Brock page."
                    : da
                      ? "Indsæt en CSV-eksport, eller kopiér opgavetabellen direkte fra Lectio."
                      : "Paste a CSV export, or copy the assignment table straight from Lectio."}
              </p>
            </div>
            <button
              onClick={() => setContent(examples[source])}
              className="text-left text-xs font-semibold text-blue-700 hover:text-blue-900"
              data-testid="import-example-button"
            >
              {da ? "Indsæt eksempeldata" : "Insert example data"} <ChevronRight className="inline size-3" />
            </button>
            <p className="flex items-start gap-2 text-xs leading-relaxed text-slate-400">
              <CheckCircle2 className="mt-0.5 size-4 shrink-0 text-emerald-500" />
              {da
                ? "Ingen skoleadgangskoder gemmes. Importen sker kun fra det, du selv indsætter eller uploader."
                : "No school passwords are stored. Import only uses what you paste or upload."}
            </p>
          </CardContent>
        </Card>
        <Card className="border-slate-200/80 shadow-sm" data-testid="import-workspace-card">
          <CardHeader className="flex flex-row items-center justify-between">
            <div>
              <CardTitle className="font-heading text-lg">
                {da ? "Importér fil eller tekst" : "Import file or text"}
              </CardTitle>
              <p className="mt-1 text-xs text-slate-500">
                {da
                  ? "CSV, JSON eller kopieret tabel · maks. 500.000 tegn"
                  : "CSV, JSON or copied table · max 500,000 characters"}
              </p>
            </div>
            <label
              className="inline-flex cursor-pointer items-center gap-2 rounded-lg border border-slate-200 bg-white px-3 py-2 text-xs font-semibold text-slate-700 hover:border-blue-200 hover:bg-blue-50"
              data-testid="import-file-label"
            >
              <Upload className="size-4 text-blue-600" /> {da ? "Vælg fil" : "Choose file"}
              <input
                type="file"
                accept=".csv,.json,.txt,text/csv,application/json,text/plain"
                onChange={(e) => {
                  const file = e.target.files?.[0];
                  if (file) void loadFile(file);
                  e.target.value = "";
                }}
                className="hidden"
                data-testid="import-file-input"
              />
            </label>
          </CardHeader>
          <CardContent className="space-y-4">
            <Textarea
              value={content}
              onChange={(e) => setContent(e.target.value)}
              placeholder={
                da
                  ? "Indsæt CSV, JSON eller en kopieret tabel her…"
                  : "Paste CSV, JSON or a copied table here…"
              }
              className="min-h-[220px] rounded-xl border-slate-200 font-mono text-xs leading-relaxed"
              data-testid="import-raw-input"
            />
            <div className="flex flex-wrap items-center justify-between gap-3">
              <p className="text-xs text-slate-400">
                {content.length.toLocaleString(da ? "da-DK" : "en-GB")} / 500.000 {da ? "tegn" : "characters"}
              </p>
              <Button
                onClick={() => preview.mutate({ source, content })}
                disabled={!content.trim() || preview.isPending}
                className="bg-blue-600 hover:bg-blue-700"
                data-testid="import-preview-button"
              >
                {preview.isPending
                  ? da
                    ? "Læser…"
                    : "Reading…"
                  : da
                    ? "Forhåndsvis import"
                    : "Preview import"}
              </Button>
            </div>
            {items.length > 0 &&
              preview.data?.warnings.map((warning) => (
                <p
                  className="rounded-lg bg-amber-50 p-3 text-xs text-amber-700"
                  key={warning}
                  data-testid="import-warning"
                >
                  {warning}
                </p>
              ))}
          </CardContent>
        </Card>
      </div>
      {items.length > 0 && (
        <Card className="border-blue-200 shadow-sm" data-testid="import-preview-card">
          <CardHeader className="flex flex-row items-center justify-between">
            <div>
              <CardTitle className="font-heading text-lg">
                {da ? "Tjek før du gemmer" : "Review before saving"}
              </CardTitle>
              <p className="mt-1 text-xs text-slate-500">
                {preview.data?.detected_format} · {selectedCount} {da ? "valgt" : "selected"}
              </p>
            </div>
            <div className="flex gap-2">
              <Button
                variant="outline"
                onClick={() => setItems(items.map((i) => ({ ...i, selected: selectedCount < items.length })))}
                data-testid="import-toggle-all-button"
              >
                {selectedCount < items.length
                  ? da
                    ? "Vælg alle"
                    : "Select all"
                  : da
                    ? "Fravælg alle"
                    : "Deselect all"}
              </Button>
              <Button
                onClick={() => commit.mutate({ source, items })}
                disabled={!selectedCount || commit.isPending}
                className="bg-blue-600 hover:bg-blue-700"
                data-testid="import-commit-button"
              >
                {commit.isPending ? (da ? "Gemmer…" : "Saving…") : da ? "Gem valgte" : "Save selected"}
              </Button>
            </div>
          </CardHeader>
          <CardContent className="space-y-2">
            {items.map((item, index) => (
              <label
                className="flex cursor-pointer items-start gap-3 rounded-xl border border-slate-100 p-3 hover:bg-slate-50"
                key={`${item.external_id}-${index}`}
                data-testid="import-preview-item"
              >
                <input
                  type="checkbox"
                  checked={item.selected}
                  onChange={() =>
                    setItems(
                      items.map((current, currentIndex) =>
                        currentIndex === index ? { ...current, selected: !current.selected } : current,
                      ),
                    )
                  }
                  className="mt-1 size-4 accent-blue-600"
                  data-testid="import-item-checkbox"
                />
                <span className="min-w-0 flex-1">
                  <span className="flex flex-wrap items-center gap-2">
                    <span className="text-sm font-semibold text-slate-800">{item.title}</span>
                    <Badge variant="outline" className="text-[10px]">
                      {item.kind === "deadline"
                        ? "Deadline"
                        : item.kind === "lesson"
                          ? da
                            ? "Lektion"
                            : "Lesson"
                          : da
                            ? "Fag"
                            : "Subject"}
                    </Badge>
                  </span>
                  <span className="mt-1 block text-xs text-slate-500">
                    {item.subject}
                    {item.date ? ` · ${dateLabel(item.date, lang)}` : item.day ? ` · ${item.day}` : ""}
                    {item.time ? ` · ${item.time}` : ""}
                    {item.end_time ? `–${item.end_time}` : ""}
                  </span>
                </span>
              </label>
            ))}
          </CardContent>
        </Card>
      )}
      <CalendarLinks lang={lang} refresh={refresh} />
      <ImportGuide lang={lang} />
    </div>
  );
}

const WEEKDAYS = ["Mandag", "Tirsdag", "Onsdag", "Torsdag", "Fredag"];
const WEEKEND = ["Lørdag", "Søndag"];
const DAY_EN: Record<string, string> = {
  Mandag: "Monday",
  Tirsdag: "Tuesday",
  Onsdag: "Wednesday",
  Torsdag: "Thursday",
  Fredag: "Friday",
  Lørdag: "Saturday",
  Søndag: "Sunday",
};
const ALL_DAYS = ["Søndag", "Mandag", "Tirsdag", "Onsdag", "Torsdag", "Fredag", "Lørdag"];

function Planner({ data, lang, refresh }: { data?: Dashboard; lang: Language; refresh: () => void }) {
  const da = lang === "da";
  const subjects = data?.subjects || [];
  const todayName = ALL_DAYS[new Date().getDay()];
  const blocksAll = data?.study_blocks || [];
  const days = [...WEEKDAYS, ...WEEKEND.filter((d) => blocksAll.some((b) => b.day === d))];
  const [form, setForm] = useState<{
    day: string;
    time: string;
    label: string;
    subject: string;
    minutes: number;
  } | null>(null);
  const plan = useMutation({
    mutationFn: () => apiPost<StudyPlan>("/study-plan"),
    onSuccess: (r) => {
      refresh();
      toast.success(da ? `Studieplan lavet: ${r.summary}` : `Study plan made: ${r.summary}`);
    },
    onError: (e) =>
      toast.error(
        e instanceof ApiError && e.status === 409
          ? da
            ? "Ingen åbne deadlines de næste 14 dage"
            : "No open deadlines in the next 14 days"
          : da
            ? "Studieplanen kunne ikke laves. Prøv igen."
            : "The study plan could not be made. Try again.",
      ),
  });
  const clear = useMutation({
    mutationFn: () => apiDelete<{ deleted: number }>("/study-plan"),
    onSuccess: () => {
      refresh();
      toast.success(da ? "Studieplan fjernet" : "Study plan removed");
    },
  });
  const addBlock = useMutation({
    mutationFn: (b: NonNullable<typeof form>) => apiPost<Block>("/blocks", b),
    onSuccess: () => {
      setForm(null);
      refresh();
      toast.success(da ? "Tilføjet til ugen" : "Added to your week");
    },
    onError: (e) => toast.error(errorDetail(e, da ? "Kunne ikke tilføjes" : "Could not be added")),
  });
  const removeBlock = useMutation({
    mutationFn: (blockId: string) => apiDelete(`/blocks/${blockId}`),
    onSuccess: refresh,
  });
  const planBlocks = blocksAll.filter((b) => b.source === "plan");
  const planMinutes = planBlocks.reduce((sum, b) => sum + (b.minutes || 0), 0);
  const sorted = [...blocksAll].sort((a, b) => a.time.localeCompare(b.time));
  const dayLabel = (d: string) => (da ? d : DAY_EN[d]);
  return (
    <div className="space-y-6" data-testid="planner-view">
      <div className="flex flex-col justify-between gap-4 sm:flex-row sm:items-end">
        <div>
          <p className="text-sm font-medium text-blue-600">{da ? "Find din rytme" : "Find your rhythm"}</p>
          <h1 className="mt-1 font-heading text-3xl font-semibold tracking-tight text-[#0b1d3a]">
            {da ? "Din uge i roligt overblik" : "Your week at a calm glance"}
          </h1>
          <p className="mt-2 text-sm text-slate-500">
            {da
              ? "Planlæg tid til både undervisning og fordybelse."
              : "Make room for both lessons and focused study."}
          </p>
        </div>
        <div className="flex flex-wrap gap-2">
          <Button
            variant="outline"
            onClick={() =>
              setForm({
                day: WEEKDAYS.includes(todayName) ? todayName : "Mandag",
                time: "16:00",
                label: "",
                subject: subjects[0]?.name ?? "Selvstudie",
                minutes: 45,
              })
            }
            data-testid="planner-add-block-button"
          >
            <Plus className="size-4" /> {da ? "Tilføj tid" : "Add time"}
          </Button>
          {planBlocks.length > 0 && (
            <Button
              variant="outline"
              onClick={() => clear.mutate()}
              disabled={clear.isPending}
              data-testid="study-plan-clear-button"
            >
              <Trash2 className="size-4" /> {da ? "Ryd plan" : "Clear plan"}
            </Button>
          )}
          <Button
            onClick={() => plan.mutate()}
            disabled={plan.isPending}
            className="bg-blue-600 hover:bg-blue-700"
            data-testid="study-plan-generate-button"
          >
            <Sparkles className={`size-4 ${plan.isPending ? "animate-pulse" : ""}`} />{" "}
            {plan.isPending
              ? da
                ? "Planlægger…"
                : "Planning…"
              : planBlocks.length
                ? da
                  ? "Lav ny plan"
                  : "New plan"
                : da
                  ? "Lav studieplan"
                  : "Make study plan"}
          </Button>
        </div>
      </div>
      {form && (
        <Card className="border-blue-200 shadow-sm">
          <CardContent className="p-4">
            <form
              className="grid gap-3 sm:grid-cols-[1fr_140px_110px_160px_auto]"
              onSubmit={(e) => {
                e.preventDefault();
                if (form.label.trim()) addBlock.mutate(form);
              }}
            >
              <Input
                autoFocus
                value={form.label}
                onChange={(e) => setForm({ ...form, label: e.target.value })}
                placeholder={
                  da ? "Hvad? Fx Fodbold eller Lektiecafé" : "What? E.g. Football or homework club"
                }
                className="h-10 rounded-xl"
                data-testid="planner-block-label-input"
              />
              <select
                value={form.day}
                onChange={(e) => setForm({ ...form, day: e.target.value })}
                className="h-10 rounded-xl border border-slate-200 bg-white px-3 text-sm"
                data-testid="planner-block-day-select"
              >
                {[...WEEKDAYS, ...WEEKEND].map((d) => (
                  <option key={d} value={d}>
                    {dayLabel(d)}
                  </option>
                ))}
              </select>
              <Input
                type="time"
                value={form.time}
                onChange={(e) => setForm({ ...form, time: e.target.value })}
                className="h-10 rounded-xl"
                data-testid="planner-block-time-input"
              />
              <select
                value={form.subject}
                onChange={(e) => setForm({ ...form, subject: e.target.value })}
                className="h-10 rounded-xl border border-slate-200 bg-white px-3 text-sm"
              >
                {subjects.map((s) => (
                  <option key={s.key} value={s.name}>
                    {s.name}
                  </option>
                ))}
              </select>
              <div className="flex gap-2">
                <Button
                  type="submit"
                  disabled={!form.label.trim() || addBlock.isPending}
                  className="h-10 bg-blue-600 hover:bg-blue-700"
                >
                  {da ? "Tilføj" : "Add"}
                </Button>
                <Button type="button" variant="ghost" className="h-10" onClick={() => setForm(null)}>
                  <X className="size-4" />
                </Button>
              </div>
            </form>
          </CardContent>
        </Card>
      )}
      {planBlocks.length > 0 && (
        <div
          className="flex items-center gap-2 rounded-xl border border-sky-100 bg-sky-50 px-4 py-3 text-sm text-sky-800"
          data-testid="study-plan-summary"
        >
          <Sparkles className="size-4 shrink-0" />{" "}
          {da
            ? `${planBlocks.length} fokusblokke · ${planMinutes} min i alt, planlagt efter dine deadlines`
            : `${planBlocks.length} focus blocks · ${planMinutes} min total, planned around your deadlines`}
        </div>
      )}
      {!blocksAll.length && (
        <div
          className="rounded-xl border border-dashed border-slate-200 bg-white px-4 py-3 text-sm text-slate-500"
          data-testid="planner-empty-hint"
        >
          {da
            ? 'Din uge er tom. Importér dit skema under "Importér fra skole", tilføj faste tider, eller lav en studieplan ud fra dine deadlines.'
            : 'Your week is empty. Import your timetable under "School import", add fixed times, or make a study plan from your deadlines.'}
        </div>
      )}
      <div className={`grid gap-3 ${days.length > 5 ? "md:grid-cols-4 xl:grid-cols-7" : "md:grid-cols-5"}`}>
        {days.map((day) => {
          const isToday = day === todayName;
          return (
            <Card
              key={day}
              className={`min-h-[280px] ${isToday ? "border-blue-300 ring-2 ring-blue-100" : ""}`}
              data-testid="study-planner-day"
            >
              <CardHeader className="flex flex-row items-center justify-between">
                <CardTitle className="font-heading text-sm">{dayLabel(day)}</CardTitle>
                {isToday && (
                  <span className="rounded-full bg-blue-600 px-2 py-0.5 text-[10px] font-semibold text-white">
                    {da ? "I dag" : "Today"}
                  </span>
                )}
              </CardHeader>
              <CardContent className="space-y-2 p-3">
                {sorted
                  .filter((b) => b.day === day)
                  .map((b) => {
                    const del = (
                      <button
                        onClick={() => removeBlock.mutate(b.id)}
                        className="absolute right-1.5 top-1.5 grid size-5 place-items-center rounded-md opacity-40 hover:bg-white/80 hover:opacity-100 group-hover:opacity-80"
                        title={da ? "Fjern" : "Remove"}
                        data-testid="planner-block-delete"
                      >
                        <X className="size-3" />
                      </button>
                    );
                    return b.source === "plan" ? (
                      <div
                        className="group relative rounded-xl border border-dashed border-sky-300 bg-sky-50 p-3 pr-6 text-sky-800"
                        key={b.id}
                        data-testid="study-plan-ai-block"
                      >
                        {del}
                        <p className="flex items-center gap-1 text-[10px] font-bold uppercase opacity-80">
                          <Sparkles className="size-3" /> {b.time} · {b.minutes} min
                        </p>
                        <p className="mt-1 text-xs font-semibold">{b.label}</p>
                        <p className="mt-0.5 text-[10px] opacity-70">{b.subject}</p>
                      </div>
                    ) : (
                      <div
                        className={`group relative rounded-xl border p-3 pr-6 ${toneFor(subjectKey(b.subject))}`}
                        key={b.id}
                        data-testid="study-planner-timeblock"
                      >
                        {del}
                        <p className="text-[10px] font-bold uppercase opacity-70">
                          {b.time}
                          {b.minutes ? ` · ${b.minutes} min` : ""}
                        </p>
                        <p className="mt-1 text-xs font-semibold">{b.label}</p>
                      </div>
                    );
                  })}
              </CardContent>
            </Card>
          );
        })}
      </div>
    </div>
  );
}

export default function Home() {
  const [lang, setLangState] = useState<Language>(() => (readStorage("kladde-language") as Language) || "da");
  const [name, setNameState] = useState(() => readStorage("kladde-name") || "");
  const [active, setActiveState] = useState<View>(() => (readStorage("kladde-view") as View) || "overview");
  const [search, setSearch] = useState("");
  const [selected, setSelected] = useState<Note | null>(null);
  const [startNewNote, setStartNewNote] = useState(false);
  const qc = useQueryClient();
  const setLang = (v: Language) => {
    setLangState(v);
    writeStorage("kladde-language", v);
    document.documentElement.lang = v;
  };
  const setName = (v: string) => {
    setNameState(v);
    writeStorage("kladde-name", v);
  };
  const setActive = (v: View) => {
    setActiveState(v);
    writeStorage("kladde-view", v);
    window.scrollTo({ top: 0 });
  };
  useEffect(() => {
    document.documentElement.lang = lang;
  }, [lang]);
  const dashboard = useQuery({
    queryKey: ["dashboard"],
    queryFn: () => apiGet<Dashboard>("/dashboard"),
    retry: 1,
  });
  const data = dashboard.data;
  useMemo(() => {
    for (const s of data?.subjects || []) subjectColors.set(s.key, s.color);
  }, [data?.subjects]);
  useEffect(() => {
    if (!selected && data?.notes[0]) setSelected(data.notes[0]);
    // Keep the open note in sync after edits elsewhere. A note that is not in the list yet was
    // just created (the list is refreshing), so it stays open; deleting clears the selection.
    if (selected && data) {
      const fresh = data.notes.find((n) => n.id === selected.id);
      if (fresh && (fresh.updated_at !== selected.updated_at || fresh.is_pinned !== selected.is_pinned))
        setSelected(fresh);
    }
  }, [data, selected]);
  // Typing in the search box from a page without a list jumps to the notes.
  const onSearch = (v: string) => {
    setSearch(v);
    if (v && (active === "overview" || active === "planner" || active === "ai" || active === "imports")) setActive("notes");
  };
  const refresh = () => {
    void qc.invalidateQueries({ queryKey: ["dashboard"] });
    void qc.invalidateQueries({ queryKey: ["weekly-summary"] });
  };
  return (
    <div className="flex min-h-svh bg-[#f8fafc] text-slate-900" data-testid="workspace-shell">
      <Sidebar
        active={active}
        setActive={setActive}
        lang={lang}
        nextTask={data?.tasks.find((x) => x.status !== "done")}
      />
      <div className="flex min-w-0 flex-1 flex-col">
        <Header
          lang={lang}
          setLang={setLang}
          setActive={setActive}
          search={search}
          setSearch={onSearch}
          reminders={data?.reminders || []}
          name={name}
          setName={setName}
        />
        <nav
          className="flex gap-2 overflow-x-auto border-b border-slate-200/80 bg-white px-4 py-2 lg:hidden"
          data-testid="mobile-navigation"
        >
          {(
            [
              ["overview", lang === "da" ? "Overblik" : "Overview"],
              ["notes", lang === "da" ? "Noter" : "Notes"],
              ["homework", lang === "da" ? "Lektier" : "Homework"],
              ["planner", lang === "da" ? "Plan" : "Planner"],
              ["ai", "AI"],
              ["imports", "Import"],
            ] as [View, string][]
          ).map(([id, label]) => (
            <button
              key={id}
              onClick={() => setActive(id)}
              className={`shrink-0 rounded-lg px-3 py-1.5 text-xs font-semibold ${active === id ? "bg-blue-50 text-blue-700" : "text-slate-500"}`}
              data-testid={`mobile-nav-${id}`}
            >
              {label}
            </button>
          ))}
        </nav>
        <main className="mx-auto w-full max-w-[1500px] flex-1 p-4 sm:p-6 lg:p-8">
          {dashboard.isError && (
            <div
              className="mb-5 flex items-center gap-3 rounded-xl border border-amber-100 bg-amber-50 p-4 text-sm text-amber-700"
              data-testid="workspace-data-error"
            >
              {lang === "da"
                ? "Dine data kunne ikke indlæses. Prøv igen, eller genstart Kladde."
                : "Your data could not load. Try again, or restart Kladde."}
              <button
                onClick={() => void dashboard.refetch()}
                className="ml-auto font-semibold underline"
                data-testid="workspace-retry-button"
              >
                {lang === "da" ? "Prøv igen" : "Retry"}
              </button>
            </div>
          )}
          {dashboard.isPending ? (
            <div className="space-y-5" data-testid="workspace-loading">
              <div className="h-10 w-72 animate-pulse rounded-xl bg-slate-200" />
              <div className="grid gap-4 sm:grid-cols-3">
                <div className="h-24 rounded-2xl bg-white" />
                <div className="h-24 rounded-2xl bg-white" />
                <div className="h-24 rounded-2xl bg-white" />
              </div>
            </div>
          ) : active === "overview" ? (
            <Overview
              data={data}
              lang={lang}
              name={name}
              setActive={setActive}
              setSelected={setSelected}
              startNewNote={() => {
                setStartNewNote(true);
                setActive("notes");
              }}
            />
          ) : active === "notes" ? (
            <Notes
              data={data}
              lang={lang}
              search={search}
              selected={selected}
              setSelected={setSelected}
              refresh={refresh}
              startNew={startNewNote}
              onStarted={() => setStartNewNote(false)}
              openAi={() => setActive("ai")}
            />
          ) : active === "homework" ? (
            <Homework data={data} lang={lang} search={search} refresh={refresh} openAi={() => setActive("ai")} />
          ) : active === "planner" ? (
            <Planner data={data} lang={lang} refresh={refresh} />
          ) : active === "ai" ? (
            <AiView lang={lang} subjects={data?.subjects || []} />
          ) : (
            <ImportCenter lang={lang} refresh={refresh} />
          )}
        </main>
        <footer
          className="flex flex-wrap items-center justify-center gap-x-4 gap-y-1 border-t border-slate-200/70 px-6 py-4 text-[11px] text-slate-400"
          data-testid="workspace-footer"
        >
          <span className="flex items-center gap-2">
            <Globe2 className="size-3.5" />
            {lang === "da"
              ? "Bygget til elever i Danmark · Kladde"
              : "Built for students in Denmark · Kladde"}
          </span>
          <button
            onClick={() =>
              exportBackup().then(
                (path) =>
                  path && toast.success(lang === "da" ? "Backup gemt" : "Backup saved", { description: path }),
                (e) =>
                  toast.error(
                    errorDetail(e, lang === "da" ? "Backuppen kunne ikke gemmes" : "The backup could not be saved"),
                  ),
              )
            }
            className="flex items-center gap-1 font-semibold text-slate-500 hover:text-blue-600"
            data-testid="backup-download-link"
          >
            <Download className="size-3.5" />
            {lang === "da" ? "Gem backup" : "Save backup"}
          </button>
        </footer>
      </div>
    </div>
  );
}
