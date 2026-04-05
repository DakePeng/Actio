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
  const diffDays = Math.floor(diffMin / 1440);

  const dayLabel = () => {
    if (diffDays === 0 || diffDays === 1) return '';
    if (diffDays <= 6) return dayNames[(now.getDay() + diffDays) % 7];
    return d.toLocaleDateString('en-US', { month: 'short', day: 'numeric' });
  };

  if (diffMin < 0) {
    const absDiffMin = Math.abs(diffMin);
    if (absDiffMin < 1440) return timeStr;
    return `Due ${dayLabel()}`;
  }
  if (diffMin < 60) return `In ${diffMin} min`;
  if (diffMin < 1440) return `${timeStr} today`;

  if (diffDays === 1) return `Tomorrow at ${timeStr}`;
  if (diffDays <= 6) return `${dayLabel()} at ${timeStr}`;
  return d.toLocaleDateString('en-US', { month: 'short', day: 'numeric' });
}
