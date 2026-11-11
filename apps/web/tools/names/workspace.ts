/**
 * The name the original gives the workspace. Here it is a workspace everywhere,
 * and that name will be a group of people's, so it is refused in any case, in
 * plurals and compounds, but where `exceptions.txt` lets it stay, each entry
 * under its reason. The server's `cargo xtask check` holds the rest of the
 * repository to a list of its own in the same form.
 *
 *   <path>             the whole file, its name too
 *   <path> | <text>    the lines of the file, where the name is within the text
 *
 * `*` in a path stands for any part of a name, `**` for any number of folders.
 * The texts are compared in any case. Of a file of messages only the ids are
 * checked: what a message says is the translators'.
 */

/** The former name, put together from halves so that this file passes its own check. */
export const FORMER = ['te', 'am'].join('');

/** The file of exceptions, from the web app's folder. */
export const EXCEPTIONS = 'tools/names/exceptions.txt';

/** An entry of the exceptions. */
export interface Exception {
  /** Its line in the file of exceptions. */
  line: number;
  /** The files it is for. */
  pattern: string;
  /** The text within which a line may have the name, lowercase; none for the whole file. */
  text: string | null;
}

/** The entries of the file of exceptions; throws on an entry without a reason. */
export function parseExceptions(listed: string): Exception[] {
  const exceptions: Exception[] = [];
  let reason = false;
  listed.split('\n').forEach((raw, index) => {
    const line = raw.trim();
    if (line === '') {
      reason = false;
      return;
    }
    if (line.startsWith('#')) {
      reason = true;
      return;
    }
    if (!reason)
      throw new Error(
        `${EXCEPTIONS}:${index + 1}: an exception needs its reason, a comment above it`,
      );
    const cut = line.indexOf(' | ');
    const pattern = cut < 0 ? line : line.slice(0, cut).trim();
    const text =
      cut < 0
        ? null
        : line
            .slice(cut + 3)
            .trim()
            .toLowerCase();
    if (text !== null && !text.includes(FORMER)) {
      throw new Error(`${EXCEPTIONS}:${index + 1}: the text has no ${FORMER} in it`);
    }
    exceptions.push({ line: index + 1, pattern, text });
  });
  return exceptions;
}

/** Whether a name is one `pattern` stands for, `*` standing for any part of it. */
function matchesName(pattern: string, name: string): boolean {
  const star = pattern.indexOf('*');
  if (star < 0) return pattern === name;
  const head = pattern.slice(0, star);
  if (!name.startsWith(head)) return false;
  const after = name.slice(head.length);
  const tail = pattern.slice(star + 1);
  for (let cut = 0; cut <= after.length; cut += 1) {
    if (matchesName(tail, after.slice(cut))) return true;
  }
  return false;
}

function matchesParts(pattern: readonly string[], path: readonly string[]): boolean {
  const [first, ...rest] = pattern;
  if (first === undefined) return path.length === 0;
  if (first === '**') {
    for (let skip = 0; skip <= path.length; skip += 1) {
      if (matchesParts(rest, path.slice(skip))) return true;
    }
    return false;
  }
  const [name, ...others] = path;
  return name !== undefined && matchesName(first, name) && matchesParts(rest, others);
}

/** Whether `path` is one of the paths `pattern` stands for. */
export function matches(pattern: string, path: string): boolean {
  return matchesParts(pattern.split('/'), path.split('/'));
}

/** The part of a line that is checked: of a file of messages, the id. */
function checkedPart(file: string, line: string): string {
  if (!/^messages\/[^/]+\.json$/.test(file)) return line;
  const value = line.indexOf('": ');
  return value < 0 ? line : line.slice(0, value + 1);
}

/**
 * The hits of one file, `path:line` (`path: in its name` for its name), marking
 * in `used` the exceptions that let a line or the name stay.
 */
export function checkFile(
  file: string,
  text: string,
  exceptions: readonly Exception[],
  used: boolean[],
): string[] {
  const mine = exceptions.flatMap((exception, index) =>
    matches(exception.pattern, file) ? [index] : [],
  );
  const whole = mine.find((index) => exceptions[index]?.text === null);
  const hits: string[] = [];
  if (file.toLowerCase().includes(FORMER)) {
    if (whole === undefined) hits.push(`${file}: in its name`);
    else used[whole] = true;
  }
  text.split('\n').forEach((line, index) => {
    let rest = checkedPart(file, line).toLowerCase();
    if (!rest.includes(FORMER)) return;
    if (whole !== undefined) {
      used[whole] = true;
      return;
    }
    for (const exception of mine) {
      const allowed = exceptions[exception]?.text;
      if (allowed && rest.includes(allowed)) {
        used[exception] = true;
        rest = rest.split(allowed).join('');
      }
    }
    if (rest.includes(FORMER)) hits.push(`${file}:${index + 1}`);
  });
  return hits;
}

/** The lines that say which exceptions nothing used. */
export function staleExceptions(
  exceptions: readonly Exception[],
  used: readonly boolean[],
): string[] {
  return exceptions
    .filter((_, index) => !used[index])
    .map((exception) => `${EXCEPTIONS}:${exception.line}: nothing needs it: remove it`);
}
