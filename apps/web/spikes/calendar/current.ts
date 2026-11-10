/** What this page was asked to show. The address does not change while the page is open. */
import { readParams } from './params.ts';

export const CURRENT = readParams(location.search);
