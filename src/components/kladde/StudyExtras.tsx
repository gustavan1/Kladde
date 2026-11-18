import { useQuery } from "@tanstack/react-query";
import { CalendarDays, CheckCircle2, CircleAlert, FileText, Timer } from "lucide-react";
import { Card, CardContent } from "@/components/ui/card";
import { apiGet } from "@/lib/api";

type Language = "da" | "en";

// Mirrors server/planner.ts (weeklySummary)
interface Task {
  id: string;
  title: string;
  subject: string;
  subject_key: string;
  deadline: string;
  status: "todo" | "doing" | "done";
  priority: "high" | "medium" | "low";
  estimated_minutes: number;
  created_at: string;
}
export interface WeeklySummary {
  week_start: string;
  is_sunday: boolean;
  completed_tasks: number;
  study_minutes: number;
  notes_touched: number;
  planned_focus_minutes: number;
  overdue_tasks: number;
  upcoming: Task[];
}
const short = (value: string, lang: Language) =>
  new Intl.DateTimeFormat(lang === "da" ? "da-DK" : "en-GB", {
    weekday: "short",
    day: "numeric",
    month: "short",
  }).format(new Date(`${value}T12:00:00`));
const hours = (minutes: number, da: boolean) =>
  (minutes >= 60
    ? `${Math.floor(minutes / 60)} t ${minutes % 60 ? `${minutes % 60} min` : ""}`.trim()
    : `${minutes} min`
  ).replace(" t", da ? " t" : " h");

export function WeeklyCard({ lang }: { lang: Language }) {
  const da = lang === "da";
  const { data } = useQuery({
    queryKey: ["weekly-summary"],
    queryFn: () => apiGet<WeeklySummary>("/weekly-summary"),
    retry: false,
  });
  if (!data) return null;
  const stats = [
    {
      icon: Timer,
      value: hours(data.study_minutes, da),
      label: da ? "studietid (færdige opgaver)" : "study time (finished tasks)",
    },
    {
      icon: CheckCircle2,
      value: String(data.completed_tasks),
      label: da ? "opgaver færdige" : "tasks finished",
    },
    {
      icon: FileText,
      value: String(data.notes_touched),
      label: da ? "noter skrevet/redigeret" : "notes written/edited",
    },
  ];
  return (
    <Card
      className={`overflow-hidden ${data.is_sunday ? "border-blue-200 bg-gradient-to-br from-blue-50 to-white shadow-md" : "border-slate-200/80"}`}
      data-testid="weekly-summary-card"
    >
      <CardContent className="p-5 sm:p-6">
        <div className="flex flex-wrap items-start justify-between gap-2">
          <div>
            <p
              className="text-xs font-semibold uppercase tracking-wide text-blue-600"
              data-testid="weekly-summary-eyebrow"
            >
              {data.is_sunday
                ? da
                  ? "Søndagsoverblik"
                  : "Sunday review"
                : da
                  ? "Din uge indtil nu"
                  : "Your week so far"}
            </p>
            <h2
              className="mt-1 font-heading text-xl font-semibold text-[#0b1d3a]"
              data-testid="weekly-summary-title"
            >
              {data.is_sunday
                ? da
                  ? "Sådan gik din uge – og det venter forude"
                  : "How your week went – and what's ahead"
                : da
                  ? `Uge fra ${short(data.week_start, lang)}`
                  : `Week from ${short(data.week_start, lang)}`}
            </h2>
          </div>
          {data.overdue_tasks > 0 && (
            <span
              className="flex items-center gap-1 rounded-full bg-red-50 px-2.5 py-1 text-xs font-semibold text-red-600"
              data-testid="weekly-overdue"
            >
              <CircleAlert className="size-3.5" />
              {data.overdue_tasks} {da ? "overskredet" : "overdue"}
            </span>
          )}
        </div>
        <div className="mt-4 grid gap-3 sm:grid-cols-3">
          {stats.map((s) => (
            <div
              key={s.label}
              className="rounded-xl border border-slate-100 bg-white p-3"
              data-testid="weekly-stat"
            >
              <s.icon className="size-4 text-blue-600" />
              <p className="mt-2 font-heading text-2xl font-semibold text-[#0b1d3a]">{s.value}</p>
              <p className="text-xs text-slate-500">{s.label}</p>
            </div>
          ))}
        </div>
        <div className="mt-4">
          <p className="mb-2 flex items-center gap-2 text-sm font-semibold text-slate-700">
            <CalendarDays className="size-4 text-blue-600" />
            {da ? "Deadlines de næste 7 dage" : "Deadlines in the next 7 days"}
          </p>
          {data.upcoming.length === 0 ? (
            <p className="text-sm text-slate-500" data-testid="weekly-upcoming-empty">
              {da ? "Ingen deadlines i den kommende uge." : "No deadlines in the coming week."}
            </p>
          ) : (
            <ul className="space-y-1.5">
              {data.upcoming.slice(0, 6).map((t) => (
                <li
                  key={t.id}
                  className="flex items-center justify-between gap-3 text-sm"
                  data-testid="weekly-upcoming-item"
                >
                  <span className="truncate text-slate-700">
                    {t.title} <span className="text-slate-400">· {t.subject}</span>
                  </span>
                  <span className="shrink-0 text-xs font-medium text-slate-500">
                    {short(t.deadline, lang)}
                  </span>
                </li>
              ))}
            </ul>
          )}
        </div>
        {data.planned_focus_minutes > 0 && (
          <p className="mt-4 text-xs text-sky-700" data-testid="weekly-focus">
            {da
              ? `Din studieplan har ${hours(data.planned_focus_minutes, da)} fokustid planlagt.`
              : `Your study plan has ${hours(data.planned_focus_minutes, da)} of focus time planned.`}
          </p>
        )}
      </CardContent>
    </Card>
  );
}
