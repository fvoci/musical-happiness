/**
 * Automatic device time-zone synchronization for course schedules and "today's learning".
 *
 * When a user's `UserSettings.timeZone` is still `"auto"`, the server evaluates
 * calendar "today" in UTC. Calling `syncDeviceTimeZone` once per workspace
 * session safely reports the browser's IANA time zone (when valid and <= 64 chars)
 * so server-side today/overdue calculations match the learner's local day.
 */
import { isTimeZone } from '../logic/regional.ts';
import { browserTimeZone, setRegionalSettings } from '../regional.ts';

import type { Workspace } from './workspace.svelte.ts';

const MAX_TIME_ZONE_LEN = 64;
const attempted = new WeakSet<Workspace>();

/**
 * Syncs `browserTimeZone()` to `UserSettings.timeZone` when it is still `"auto"`.
 * Runs at most once per `Workspace` instance so an explicit user switch back to
 * `"auto"` in Settings is never overwritten. Returns `true` if updated.
 */
export async function syncDeviceTimeZone(workspace: Workspace): Promise<boolean> {
  if (attempted.has(workspace)) return false;
  attempted.add(workspace);
  if (workspace.settings.timeZone !== 'auto' || !workspace.me?.id) return false;
  const zone = browserTimeZone().trim();
  if (zone === '' || zone === 'auto' || zone.length > MAX_TIME_ZONE_LEN || !isTimeZone(zone)) {
    return false;
  }
  try {
    const saved = await workspace.backend.patchSettings({ timeZone: zone });
    workspace.settings = saved;
    setRegionalSettings(saved);
    return true;
  } catch {
    // Non-fatal: e.g. read-only backend or unrecognized alias; keep current settings quietly.
    return false;
  }
}
