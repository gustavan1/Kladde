export function formatStudyDate(value: string, lang: "da" | "en", time?: string | null, weekday = false) {
  const date = new Date(`${value}T12:00:00`);
  const valid = /^\d{4}-\d{2}-\d{2}$/.test(value) && Number.isFinite(date.getTime())
    && date.getFullYear() === Number(value.slice(0, 4))
    && date.getMonth() + 1 === Number(value.slice(5, 7))
    && date.getDate() === Number(value.slice(8, 10));
  if (!valid) return lang === "da" ? "Ukendt dato" : "Unknown date";
  const label = new Intl.DateTimeFormat(lang === "da" ? "da-DK" : "en-GB", {
    weekday: weekday ? "short" : undefined, day: "numeric", month: "short",
  }).format(date);
  return time ? `${label} · ${time}` : label;
}
