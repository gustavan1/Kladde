export function formatStudyDate(value: string, lang: "da" | "en", time?: string | null, weekday = false) {
  const date = new Date(`${value}T12:00:00`);
  if (!Number.isFinite(date.getTime())) return lang === "da" ? "Ukendt dato" : "Unknown date";
  const label = new Intl.DateTimeFormat(lang === "da" ? "da-DK" : "en-GB", {
    weekday: weekday ? "short" : undefined, day: "numeric", month: "short",
  }).format(date);
  return time ? `${label} · ${time}` : label;
}
