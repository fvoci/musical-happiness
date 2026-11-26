/**
 * The time zone of who leaves it to follow the device ("Automatic").
 *
 * The server reckons the reader's days (a course's today and overdue, the hub
 * of today, the reminders) in `UserSettings.timeZone`, and in UTC while that
 * is `"auto"`. So the zone a reader follows the device for is the device's,
 * told to the server, and the choice itself is kept apart, in the state the
 * client keeps with the account (`FOLLOWS_DEVICE`): the settings show
 * "Automatic" still, and a device in another zone tells its own once it is
 * entered. A zone picked by hand is kept as it is, wherever the reader is.
 */
import { isTimeZone } from '../logic/regional.ts';
import { browserTimeZone, setRegionalSettings } from '../regional.ts';

import type { Workspace } from './workspace.svelte.ts';

/** The entry of the client's state that says the reader follows the device's time zone. */
export const FOLLOWS_DEVICE = 'timeZoneFollowsDevice';
const MAX_TIME_ZONE_LEN = 64;

class DeviceZone {
  /** The reader follows the device, as last read or chosen here. */
  follows = $state(false);
  /** Told once for each `Workspace`: what another device tells meanwhile is not told back. */
  synced = false;
}

const ofWorkspace = new WeakMap<Workspace, DeviceZone>();

function zoneOf(workspace: Workspace): DeviceZone {
  let zone = ofWorkspace.get(workspace);
  if (!zone) {
    zone = new DeviceZone();
    ofWorkspace.set(workspace, zone);
  }
  return zone;
}

/** The device's zone where the server takes it: an IANA name of at most 64 letters. */
function deviceZone(): string | undefined {
  const zone = browserTimeZone().trim();
  if (zone === '' || zone === 'auto' || zone.length > MAX_TIME_ZONE_LEN || !isTimeZone(zone)) {
    return undefined;
  }
  return zone;
}

/** Whether the reader follows the device's time zone: chosen so, or never set. */
export function followsDeviceTimeZone(workspace: Workspace): boolean {
  return zoneOf(workspace).follows || workspace.settings.timeZone === 'auto';
}

/**
 * Tells the server the device's zone where the reader follows it (chosen so,
 * or never set: then the choice is kept), once for each `Workspace`. Returns
 * whether the settings were changed.
 */
export async function syncDeviceTimeZone(workspace: Workspace): Promise<boolean> {
  const state = zoneOf(workspace);
  if (state.synced || !workspace.me?.id) return false;
  state.synced = true;
  const { backend } = workspace;
  try {
    const unset = workspace.settings.timeZone === 'auto';
    state.follows = unset || (await backend.screenState())[FOLLOWS_DEVICE] === '1';
    const zone = deviceZone();
    if (!state.follows || zone === undefined || zone === workspace.settings.timeZone) return false;
    const saved = await backend.patchSettings({ timeZone: zone });
    if (unset) await backend.patchScreenState({ [FOLLOWS_DEVICE]: '1' });
    workspace.settings = saved;
    setRegionalSettings(saved);
    return true;
  } catch {
    // Non-fatal: e.g. a read-only backend; the settings stay as they are, quietly.
    return false;
  }
}

/**
 * The reader picks a time zone: `'auto'` follows the device (its zone is told
 * to the server), another is kept as it is. Shown at once; a change refused
 * goes back and is told, as any change of the settings.
 */
export async function chooseTimeZone(workspace: Workspace, timeZone: string): Promise<void> {
  const state = zoneOf(workspace);
  const before = state.follows;
  const follows = timeZone === 'auto';
  const wanted = follows ? (deviceZone() ?? 'auto') : timeZone;
  state.follows = follows;
  await workspace.changeSettings({ timeZone: wanted });
  if (workspace.settings.timeZone !== wanted) {
    state.follows = before;
    return;
  }
  try {
    if (follows) await workspace.backend.patchScreenState({ [FOLLOWS_DEVICE]: '1' });
    else if (before) await workspace.backend.patchScreenState({}, [FOLLOWS_DEVICE]);
  } catch (error) {
    workspace.report('save', error);
  }
}
