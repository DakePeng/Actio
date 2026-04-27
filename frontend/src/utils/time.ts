export function formatTimeShort(dateStr: string): string {
  const d = new Date(dateStr);
  const now = new Date();
  const diffMin = Math.round((d.getTime() - now.getTime()) / 60000);

  const hours = d.getHours();
  const mins = d.getMinutes().toString().padStart(2, '0');
  const ampm = hours >= 12 ? 'PM' : 'AM';
  const h = hours % 12 || 12;
  const timeStr = `${h}:${mins} ${ampm}`;

  const dayNames = ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday'];

  // Calendar-day diff (positive = future, negative = past). Computing
  // it from local-midnight anchors of each side avoids the time-bucket
  // pitfalls of `floor(diffMin/1440)` — see ISSUES.md #72 for context.
  const startOfDay = (date: Date) =>
    new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime();
  const calDiffDays = Math.round(
    (startOfDay(d) - startOfDay(now)) / 86_400_000,
  );

  if (diffMin < 0) {
    if (calDiffDays === 0) return timeStr;
    if (calDiffDays >= -6) return `Due ${dayNames[d.getDay()]}`;
    return `Due ${d.toLocaleDateString('en-US', { month: 'short', day: 'numeric' })}`;
  }
  if (diffMin < 60) return `In ${diffMin} min`;
  if (calDiffDays === 0) return `${timeStr} today`;
  if (calDiffDays === 1) return `Tomorrow at ${timeStr}`;
  if (calDiffDays <= 6) return `${dayNames[d.getDay()]} at ${timeStr}`;
  return d.toLocaleDateString('en-US', { month: 'short', day: 'numeric' });
}
