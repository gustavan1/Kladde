import { useEffect, useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import {
  CalendarDays,
  Check,
  ListChecks,
  Send,
  Settings2,
  Sparkles,
  Trash2,
  Wand2,
  X,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { apiDelete, apiGet, apiPost, errorDetail } from "@/lib/api";

import { AiSetup, useAiStatus, ClaudePanel } from "./Providers";

type Language = "da" | "en";
type Subject = { id: string; name: string; key: string; color: string };
type Message = { id: string; role: "user" | "assistant"; content: string; subject: string; created_at: string };
type PlanStep = { title: string; deadline: string; estimated_minutes: number; priority: "high" | "medium" | "low" };

/** The AI helper page: chat about anything, with your deadlines (and optionally notes) as context. */
export function AiView({ lang, subjects }: { lang: Language; subjects: Subject[] }) {
  const da = lang === "da";
  const qc = useQueryClient();
  const status = useAiStatus();
  const [prompt, setPrompt] = useState("");
  const [subject, setSubject] = useState("Generelt");
  const [showSettings, setShowSettings] = useState(false);
  const listRef = useRef<HTMLDivElement>(null);
  const configured = Boolean(status.data?.configured);
  const history = useQuery({
    queryKey: ["ai-messages"],
    queryFn: () => apiGet<Message[]>("/ai/messages"),
    enabled: configured,
  });
  const send = useMutation({
    mutationFn: (p: { prompt: string; subject: string }) => apiPost<Message>("/ai/chat", p),
    onSuccess: () => {
      setPrompt("");
      void qc.invalidateQueries({ queryKey: ["ai-messages"] });
    },
    onError: (e) => toast.error(errorDetail(e, da ? "AI-hjælperen kunne ikke svare lige nu." : "The AI helper could not answer right now.")),
  });
  const clear = useMutation({
    mutationFn: () => apiDelete("/ai/messages"),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["ai-messages"] }),
  });
  useEffect(() => {
    listRef.current?.scrollTo({ top: listRef.current.scrollHeight, behavior: "smooth" });
  }, [history.data, send.isPending]);
  const submit = () => {
    if (prompt.trim() && !send.isPending) send.mutate({ prompt: prompt.trim(), subject });
  };
  const quick = da
    ? ["Hvad skal jeg lave i dag?", "Lav en plan for mine deadlines denne uge", "Forklar det som til en 1.g-elev", "Lav 5 eksamensspørgsmål"]
    : ["What should I do today?", "Plan my deadlines this week", "Explain it like I'm a first-year student", "Make 5 exam questions"];
  const messages = history.data || [];
  return (
    <div className="space-y-6" data-testid="ai-view">
      <div className="flex flex-col justify-between gap-4 sm:flex-row sm:items-end">
        <div>
          <p className="flex items-center gap-2 text-sm font-medium text-blue-600">
            <Sparkles className="size-4" /> LektieHjælper{configured && ` · ${status.data?.provider === "codex" ? "ChatGPT" : "Cloudflare"}`}
          </p>
          <h1 className="mt-1 font-heading text-3xl font-semibold tracking-tight text-[#0b1d3a]">
            {da ? "AI-hjælper" : "AI helper"}
          </h1>
          <p className="mt-2 max-w-2xl text-sm text-slate-500" data-testid="ai-intro">
            {da
              ? "Spørg om stoffet, få hjælp til at planlægge dine deadlines, eller få noget forklaret trin for trin. Hjælperen kan se dine lektier og frister."
              : "Ask about your subjects, get help planning your deadlines, or have something explained step by step. The helper can see your homework and deadlines."}
          </p>
        </div>
        {configured && (
          <Button variant="outline" onClick={() => setShowSettings(!showSettings)} data-testid="ai-settings-button">
            <Settings2 className="size-4" /> {da ? "AI-indstillinger" : "AI settings"}
          </Button>
        )}
      </div>
      {status.data?.mac && <ClaudePanel lang={lang} />}
      {!configured ? (
        <AiSetup lang={lang} />
      ) : (
        <>
          {showSettings && (
            <Card className="border-slate-200/80">
              <CardContent className="p-4">
                <AiSetup lang={lang} />
              </CardContent>
            </Card>
          )}
          <Card className="overflow-hidden border-blue-100 shadow-sm" data-testid="ai-chat-card">
            <CardHeader className="flex flex-row items-center justify-between gap-3 border-b border-blue-50 bg-blue-50/60">
              <div className="flex items-center gap-3">
                <span className="grid size-10 place-items-center rounded-xl bg-blue-600 text-white">
                  <Sparkles className="size-5" />
                </span>
                <CardTitle className="font-heading text-lg">{da ? "Din studiepartner" : "Your study partner"}</CardTitle>
              </div>
              <div className="flex items-center gap-2">
                <select
                  value={subject}
                  onChange={(e) => setSubject(e.target.value)}
                  className="h-9 rounded-lg border border-blue-100 bg-white px-2 text-xs"
                  title={da ? "Vælg et fag, så kan hjælperen læse dine noter i faget" : "Pick a subject so the helper can read your notes for it"}
                  data-testid="ai-subject-select"
                >
                  <option value="Generelt">{da ? "Alle fag (uden noter)" : "All subjects (no notes)"}</option>
                  {subjects.map((s) => (
                    <option value={s.name} key={s.id}>
                      {da ? `${s.name} + mine noter` : `${s.name} + my notes`}
                    </option>
                  ))}
                </select>
                {messages.length > 0 && (
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    onClick={() => clear.mutate()}
                    title={da ? "Ryd samtalen" : "Clear the conversation"}
                    data-testid="ai-clear-chat"
                  >
                    <Trash2 className="size-4 text-slate-400" />
                  </Button>
                )}
              </div>
            </CardHeader>
            <CardContent className="flex min-h-[460px] flex-col p-0">
              <div ref={listRef} className="max-h-[60vh] flex-1 space-y-4 overflow-y-auto p-5" data-testid="ai-message-list">
                {!messages.length && (
                  <div className="flex min-h-[260px] flex-col items-center justify-center text-center">
                    <span className="grid size-12 place-items-center rounded-2xl bg-blue-50 text-blue-600">
                      <Sparkles className="size-5" />
                    </span>
                    <p className="mt-4 font-heading text-lg font-semibold">
                      {da ? "Hvad vil du gerne have hjælp til?" : "What would you like help with?"}
                    </p>
                    <p className="mt-2 max-w-sm text-sm text-slate-500">
                      {da ? "Jeg hjælper dig med at forstå stoffet – ikke bare med at kopiere et svar." : "I help you understand the material — not just copy an answer."}
                    </p>
                  </div>
                )}
                {messages.map((m) => (
                  <div key={m.id} className={`flex ${m.role === "user" ? "justify-end" : "justify-start"}`} data-testid={`ai-message-${m.role}`}>
                    <div
                      className={`max-w-[85%] whitespace-pre-wrap rounded-2xl px-4 py-3 text-sm leading-relaxed ${m.role === "user" ? "rounded-br-md bg-blue-600 text-white" : "rounded-bl-md border border-slate-200 bg-slate-50 text-slate-700"}`}
                    >
                      {m.content}
                    </div>
                  </div>
                ))}
                {send.isPending && (
                  <div className="flex items-center gap-2 text-sm text-slate-400" data-testid="ai-typing-indicator">
                    <Sparkles className="size-4 animate-pulse text-blue-500" /> {da ? "Tænker…" : "Thinking…"}
                  </div>
                )}
              </div>
              <div className="border-t border-slate-100 p-4">
                <div className="mb-3 flex flex-wrap gap-2">
                  {quick.map((q) => (
                    <button
                      key={q}
                      onClick={() => setPrompt(q)}
                      className="rounded-full border border-blue-100 bg-blue-50/60 px-3 py-1.5 text-xs font-medium text-blue-700 hover:bg-blue-100"
                      data-testid="ai-quick-prompt-chip"
                    >
                      {q}
                    </button>
                  ))}
                </div>
                <div className="flex items-end gap-2">
                  <Textarea
                    value={prompt}
                    onChange={(e) => setPrompt(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter" && !e.shiftKey) {
                        e.preventDefault();
                        submit();
                      }
                    }}
                    placeholder={da ? "Skriv dit spørgsmål… (Enter sender, Shift+Enter giver ny linje)" : "Type your question… (Enter sends, Shift+Enter for a new line)"}
                    className="min-h-12 resize-none rounded-xl"
                    data-testid="ai-chat-input"
                  />
                  <Button
                    onClick={submit}
                    disabled={!prompt.trim() || send.isPending}
                    className="size-11 rounded-xl bg-blue-600 p-0 hover:bg-blue-700"
                    title={da ? "Send" : "Send"}
                    data-testid="ai-chat-send-btn"
                  >
                    <Send className="size-4" />
                  </Button>
                </div>
              </div>
            </CardContent>
          </Card>
        </>
      )}
    </div>
  );
}

type NoteAction = "organize" | "summary" | "from_text" | "explain" | "questions";

/** AI buttons for a note. In the editor the result can replace or extend the text; when
 *  reading a note it can be added to the note or saved as a new note. */
export function NoteAiTools({
  lang,
  mode,
  title,
  content,
  subject,
  onUse,
  onSetup,
}: {
  lang: Language;
  mode: "edit" | "view";
  title: string;
  content: string;
  subject: string;
  onUse: (text: string, how: "replace" | "append" | "new") => void;
  onSetup: () => void;
}) {
  const da = lang === "da";
  const status = useAiStatus();
  const [result, setResult] = useState<{ action: NoteAction; text: string } | null>(null);
  const run = useMutation({
    mutationFn: (action: NoteAction) => apiPost<{ text: string }>("/ai/note", { action, title, content, subject }),
    onSuccess: (r, action) => setResult({ action, text: r.text }),
    onError: (e) => toast.error(errorDetail(e, da ? "AI kunne ikke hjælpe lige nu." : "AI could not help right now.")),
  });
  const actions: [NoteAction, string][] =
    mode === "edit"
      ? [
          ["organize", da ? "Organisér noten" : "Organise note"],
          ["from_text", da ? "Lav noter af indsat tekst" : "Make notes from pasted text"],
          ["summary", da ? "Opsummér" : "Summarise"],
        ]
      : [
          ["summary", da ? "Opsummér" : "Summarise"],
          ["explain", da ? "Forklar det svære" : "Explain the hard parts"],
          ["questions", da ? "Øvelsesspørgsmål" : "Practice questions"],
        ];
  if (!status.data?.configured) {
    return (
      <button
        type="button"
        onClick={onSetup}
        className="flex items-center gap-1.5 text-xs font-semibold text-blue-600 hover:text-blue-800"
        data-testid="note-ai-setup-link"
      >
        <Wand2 className="size-3.5" /> {da ? "Få hjælp af AI til dine noter – slå AI til" : "Get AI help with your notes – switch on AI"}
      </button>
    );
  }
  return (
    <div className="space-y-3 rounded-2xl border border-blue-100 bg-blue-50/40 p-3" data-testid="note-ai-tools">
      <div className="flex flex-wrap items-center gap-2">
        <span className="flex items-center gap-1.5 text-xs font-semibold text-blue-800">
          <Wand2 className="size-3.5" /> AI
        </span>
        {actions.map(([action, label]) => (
          <button
            key={action}
            type="button"
            disabled={run.isPending || content.trim().length < 20}
            onClick={() => run.mutate(action)}
            className="rounded-full border border-blue-100 bg-white px-3 py-1 text-xs font-medium text-blue-700 hover:bg-blue-100 disabled:opacity-50"
            title={content.trim().length < 20 ? (da ? "Skriv lidt mere først" : "Write a bit more first") : undefined}
            data-testid={`note-ai-${action}`}
          >
            {label}
          </button>
        ))}
        {run.isPending && (
          <span className="flex items-center gap-1 text-xs text-slate-500">
            <Sparkles className="size-3.5 animate-pulse text-blue-500" /> {da ? "Arbejder…" : "Working…"}
          </span>
        )}
      </div>
      {result && (
        <div className="space-y-2 rounded-xl border border-slate-200 bg-white p-3" data-testid="note-ai-result">
          <div className="max-h-72 overflow-y-auto whitespace-pre-wrap text-sm leading-relaxed text-slate-700">{result.text}</div>
          <div className="flex flex-wrap justify-end gap-2 border-t border-slate-100 pt-2">
            <Button type="button" variant="ghost" size="sm" onClick={() => setResult(null)}>
              <X className="size-3.5" /> {da ? "Kassér" : "Discard"}
            </Button>
            {mode === "edit" ? (
              <>
                <Button type="button" variant="outline" size="sm" onClick={() => { onUse(result.text, "append"); setResult(null); }} data-testid="note-ai-append">
                  {da ? "Tilføj nederst" : "Add at the end"}
                </Button>
                <Button type="button" size="sm" className="bg-blue-600 hover:bg-blue-700" onClick={() => { onUse(result.text, "replace"); setResult(null); }} data-testid="note-ai-replace">
                  <Check className="size-3.5" /> {da ? "Erstat teksten" : "Replace the text"}
                </Button>
              </>
            ) : (
              <>
                <Button type="button" variant="outline" size="sm" onClick={() => { onUse(result.text, "new"); setResult(null); }} data-testid="note-ai-new">
                  {da ? "Gem som ny note" : "Save as new note"}
                </Button>
                <Button type="button" size="sm" className="bg-blue-600 hover:bg-blue-700" onClick={() => { onUse(result.text, "append"); setResult(null); }} data-testid="note-ai-append">
                  <Check className="size-3.5" /> {da ? "Tilføj til noten" : "Add to the note"}
                </Button>
              </>
            )}
          </div>
          <p className="text-[11px] text-slate-400">
            {da ? "Tjek altid AI'ens tekst – den kan tage fejl." : "Always check the AI's text – it can make mistakes."}
          </p>
        </div>
      )}
    </div>
  );
}

/** Splits a project into steps with their own deadlines; chosen steps become tasks. */
export function ProjectPlanner({
  lang,
  subjects,
  today,
  onClose,
  onCreated,
  onSetup,
}: {
  lang: Language;
  subjects: Subject[];
  today: string;
  onClose: () => void;
  onCreated: () => void;
  onSetup: () => void;
}) {
  const da = lang === "da";
  const status = useAiStatus();
  const [title, setTitle] = useState("");
  const [details, setDetails] = useState("");
  const [subject, setSubject] = useState(subjects[0]?.name ?? "Dansk");
  const [deadline, setDeadline] = useState(today);
  const [steps, setSteps] = useState<(PlanStep & { selected: boolean })[]>([]);
  const [advice, setAdvice] = useState("");
  const plan = useMutation({
    mutationFn: () => apiPost<{ steps: PlanStep[]; advice: string }>("/ai/project-plan", { title, details, subject, deadline }),
    onSuccess: (r) => {
      setSteps(r.steps.map((s) => ({ ...s, selected: true })));
      setAdvice(r.advice);
    },
    onError: (e) => toast.error(errorDetail(e, da ? "Planen kunne ikke laves." : "The plan could not be made.")),
  });
  const create = useMutation({
    mutationFn: async () => {
      const chosen = steps.filter((s) => s.selected);
      for (const s of chosen)
        await apiPost("/tasks", { title: `${s.title}`, subject, deadline: s.deadline, priority: s.priority, estimated_minutes: s.estimated_minutes });
      return chosen.length;
    },
    onSuccess: (n) => {
      toast.success(da ? `${n} opgaver tilføjet` : `${n} tasks added`);
      onCreated();
      onClose();
    },
    onError: (e) => toast.error(errorDetail(e, da ? "Opgaverne kunne ikke gemmes" : "The tasks could not be saved")),
  });
  const fmt = (d: string) =>
    new Intl.DateTimeFormat(da ? "da-DK" : "en-GB", { weekday: "short", day: "numeric", month: "short" }).format(new Date(`${d}T12:00:00`));
  const prio = { high: da ? "Høj" : "High", medium: da ? "Mellem" : "Medium", low: da ? "Lav" : "Low" };
  return (
    <Card className="border-blue-200 shadow-lg" data-testid="project-planner">
      <CardHeader className="flex flex-row items-center justify-between">
        <CardTitle className="flex items-center gap-2 font-heading">
          <ListChecks className="size-4 text-blue-600" /> {da ? "Planlæg et projekt med AI" : "Plan a project with AI"}
        </CardTitle>
        <button onClick={onClose} className="grid size-8 place-items-center rounded-lg text-slate-400 hover:bg-slate-100">
          <X className="size-4" />
        </button>
      </CardHeader>
      <CardContent className="space-y-4">
        {!status.data?.configured ? (
          <div className="rounded-xl bg-blue-50 p-4 text-sm text-blue-800">
            {da ? "AI er ikke slået til endnu. " : "AI is not switched on yet. "}
            <button onClick={onSetup} className="font-semibold underline" data-testid="project-ai-setup-link">
              {da ? "Slå AI til" : "Switch on AI"}
            </button>
          </div>
        ) : steps.length === 0 ? (
          <form
            className="grid gap-3 sm:grid-cols-2"
            onSubmit={(e) => {
              e.preventDefault();
              if (title.trim() && !plan.isPending) plan.mutate();
            }}
          >
            <Input
              autoFocus
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              placeholder={da ? "Projekt, fx SRP om klimaforandringer" : "Project, e.g. history essay on WW2"}
              className="h-10 rounded-xl sm:col-span-2"
              data-testid="project-title-input"
            />
            <label className="space-y-1 text-xs font-medium text-slate-500">
              {da ? "Fag" : "Subject"}
              <select
                value={subject}
                onChange={(e) => setSubject(e.target.value)}
                className="h-10 w-full rounded-xl border border-slate-200 bg-white px-3 text-sm text-slate-900"
                data-testid="project-subject-select"
              >
                {subjects.map((s) => (
                  <option key={s.key} value={s.name}>
                    {s.name}
                  </option>
                ))}
              </select>
            </label>
            <label className="space-y-1 text-xs font-medium text-slate-500">
              {da ? "Endelig aflevering" : "Final due date"}
              <Input
                type="date"
                min={today}
                value={deadline}
                onChange={(e) => setDeadline(e.target.value)}
                className="h-10 rounded-xl text-slate-900"
                data-testid="project-deadline-input"
              />
            </label>
            <Textarea
              value={details}
              onChange={(e) => setDetails(e.target.value)}
              placeholder={da ? "Detaljer (valgfrit): krav, sidetal, hvad du allerede har lavet…" : "Details (optional): requirements, length, what you have already done…"}
              className="min-h-20 rounded-xl sm:col-span-2"
              data-testid="project-details-input"
            />
            <div className="flex justify-end gap-2 sm:col-span-2">
              <Button type="button" variant="outline" onClick={onClose}>
                {da ? "Annuller" : "Cancel"}
              </Button>
              <Button type="submit" disabled={!title.trim() || plan.isPending} className="bg-blue-600 hover:bg-blue-700" data-testid="project-plan-button">
                <Sparkles className={`size-4 ${plan.isPending ? "animate-pulse" : ""}`} />
                {plan.isPending ? (da ? "Planlægger…" : "Planning…") : da ? "Lav plan" : "Make plan"}
              </Button>
            </div>
          </form>
        ) : (
          <div className="space-y-3">
            {advice && <p className="rounded-xl bg-sky-50 p-3 text-sm leading-relaxed text-sky-800" data-testid="project-advice">{advice}</p>}
            <div className="space-y-2">
              {steps.map((s, i) => (
                <label key={i} className="flex cursor-pointer items-start gap-3 rounded-xl border border-slate-100 p-3 hover:bg-slate-50" data-testid="project-step">
                  <input
                    type="checkbox"
                    checked={s.selected}
                    onChange={() => setSteps(steps.map((x, j) => (j === i ? { ...x, selected: !x.selected } : x)))}
                    className="mt-1 size-4 accent-blue-600"
                  />
                  <span className="min-w-0 flex-1">
                    <span className="block text-sm font-semibold text-slate-800">{s.title}</span>
                    <span className="mt-1 flex flex-wrap items-center gap-3 text-xs text-slate-500">
                      <span className="flex items-center gap-1">
                        <CalendarDays className="size-3.5" /> {fmt(s.deadline)}
                      </span>
                      <span>{s.estimated_minutes} min</span>
                      <span className={s.priority === "high" ? "font-semibold text-red-600" : s.priority === "medium" ? "text-amber-600" : ""}>{prio[s.priority]}</span>
                    </span>
                  </span>
                </label>
              ))}
            </div>
            <div className="flex flex-wrap justify-end gap-2">
              <Button variant="outline" onClick={() => setSteps([])}>
                {da ? "Ret og prøv igen" : "Edit and retry"}
              </Button>
              <Button
                onClick={() => create.mutate()}
                disabled={!steps.some((s) => s.selected) || create.isPending}
                className="bg-blue-600 hover:bg-blue-700"
                data-testid="project-create-tasks"
              >
                <Check className="size-4" /> {da ? "Tilføj valgte som opgaver" : "Add selected as tasks"}
              </Button>
            </div>
          </div>
        )}
      </CardContent>
    </Card>
  );
}
