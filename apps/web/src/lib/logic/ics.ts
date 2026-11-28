/**
 * An iCalendar file (RFC 5545) of dated things: one `VEVENT` each, with a
 * `UID` of its own that stays the same from one file to the next (so that a
 * calendar the file is opened in again updates its events in place of adding
 * them twice), the `DTSTAMP` of the file, and its days or moments.
 *
 * A day is a `VALUE=DATE`, and the end of days is the day after the last of
 * them, as RFC 5545 has it (`DTEND` is not part of the event): a card of the
 * 3rd to the 5th ends on the 6th. A moment is written in UTC (`Z`). A text
 * is escaped (`\\`, `;`, `,`, a new line) and a line longer than 75 octets
 * is folded, never inside a character of UTF-8.
 */
import { addDays, type CalendarDate, type DateValue } from './dates.ts';

/** One event of the file. */
export interface IcsEvent {
  /** Stays the same for the same thing: a card's identifier, say. */
  readonly uid: string;
  readonly title: string;
  readonly when: DateValue;
  /** A link to the thing, if it has one. */
  readonly url?: string | undefined;
}

const encoder = new TextEncoder();

/** A text as a value of iCalendar has it. */
export function icsText(text: string): string {
  return text
    .replace(/\\/g, '\\\\')
    .replace(/;/g, '\\;')
    .replace(/,/g, '\\,')
    .replace(/\r\n|\r|\n/g, '\\n');
}

/** A line folded at 75 octets, a fold never inside a character. */
export function foldLine(line: string): string {
  const parts: string[] = [];
  let part = '';
  let octets = 0;
  for (const character of line) {
    const size = encoder.encode(character).length;
    // The first line holds 75 octets, each after it 74 after the space it starts with.
    const room = parts.length === 0 ? 75 : 74;
    if (octets + size > room) {
      parts.push(part);
      part = '';
      octets = 0;
    }
    part += character;
    octets += size;
  }
  parts.push(part);
  return parts.join('\r\n ');
}

const pad = (value: number, width = 2): string => String(value).padStart(width, '0');

function dayOf(date: CalendarDate): string {
  return date.replace(/-/g, '');
}

function momentOf(instant: number): string {
  const at = new Date(instant);
  return (
    `${pad(at.getUTCFullYear(), 4)}${pad(at.getUTCMonth() + 1)}${pad(at.getUTCDate())}` +
    `T${pad(at.getUTCHours())}${pad(at.getUTCMinutes())}${pad(at.getUTCSeconds())}Z`
  );
}

/** The lines of when an event is, or none for a value without a start. */
function whenLines(when: DateValue): string[] | undefined {
  const { from, to } = when;
  if (!from) return undefined;
  if (from.date !== undefined) {
    const last = to?.date ?? from.date;
    return [
      `DTSTART;VALUE=DATE:${dayOf(from.date)}`,
      `DTEND;VALUE=DATE:${dayOf(addDays(last, 1))}`,
    ];
  }
  const start = `DTSTART:${momentOf(from.instant)}`;
  return to?.instant === undefined ? [start] : [start, `DTEND:${momentOf(to.instant)}`];
}

/** The file of `events` made at `now`, named `name` in a calendar it is added to. */
export function icsCalendar(name: string, events: readonly IcsEvent[], now: number): string {
  const lines = [
    'BEGIN:VCALENDAR',
    'VERSION:2.0',
    'PRODID:-//fvoci//fvoci//EN',
    'CALSCALE:GREGORIAN',
    `X-WR-CALNAME:${icsText(name)}`,
  ];
  const stamp = momentOf(now);
  for (const event of events) {
    const when = whenLines(event.when);
    if (!when) continue;
    lines.push(
      'BEGIN:VEVENT',
      `UID:${icsText(event.uid)}`,
      `DTSTAMP:${stamp}`,
      ...when,
      `SUMMARY:${icsText(event.title)}`,
    );
    if (event.url) lines.push(`URL:${event.url}`);
    lines.push('END:VEVENT');
  }
  lines.push('END:VCALENDAR');
  return `${lines.map(foldLine).join('\r\n')}\r\n`;
}
