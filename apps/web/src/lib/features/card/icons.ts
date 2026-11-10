/**
 * The icons of a card detail: an emoji to give a card that has none, and the
 * small picture that goes with each kind of property.
 *
 * "Add icon" gives the card one emoji at random, as the original did. The
 * list is this file's own and is short on purpose: a picker over the full
 * emoji list is being built elsewhere, and when it is there the card opens
 * it instead (CardTitle.svelte is the one place that asks for an emoji).
 */
import AtSignIcon from '@lucide/svelte/icons/at-sign';
import CalendarIcon from '@lucide/svelte/icons/calendar';
import CircleDotIcon from '@lucide/svelte/icons/circle-dot';
import ClockIcon from '@lucide/svelte/icons/clock';
import HashIcon from '@lucide/svelte/icons/hash';
import LinkIcon from '@lucide/svelte/icons/link';
import PaperclipIcon from '@lucide/svelte/icons/paperclip';
import PhoneIcon from '@lucide/svelte/icons/phone';
import SquareCheckIcon from '@lucide/svelte/icons/square-check';
import TagsIcon from '@lucide/svelte/icons/tags';
import TypeIcon from '@lucide/svelte/icons/type';
import UserIcon from '@lucide/svelte/icons/user';
import UserPenIcon from '@lucide/svelte/icons/user-pen';
import UsersIcon from '@lucide/svelte/icons/users';
import type { Component } from 'svelte';

import type { PropertyType } from '$lib/logic/wire.ts';

/**
 * Things, plants, food and weather: nothing that reads as a judgement on the
 * card. Every one is drawn in colour without a variation selector.
 */
// prettier-ignore
export const CARD_EMOJI: readonly string[] = [
  '🌱', '🌿', '🍀', '🌵', '🌲', '🌳', '🌴', '🌻', '🌼', '🌷',
  '🍁', '🍂', '🍄', '🌊', '🌙', '⭐', '🌈', '🔥', '⚡', '⛄',
  '🍎', '🍊', '🍋', '🍉', '🍇', '🍓', '🍒', '🍑', '🥝', '🥑',
  '🥕', '🌽', '🍞', '🧀', '🍪', '🍰', '🍵', '☕', '🍙', '🍜',
  '🎈', '🎁', '🎨', '🎬', '🎧', '🎹', '🎸', '🎲', '🎯', '🏆',
  '⚽', '🏀', '🎾', '🏓', '🚲', '🚗', '🚀', '⛵', '🏠', '⛺',
  '📌', '📎', '📐', '📚', '📖', '📝', '📦', '📫', '📷', '💡',
  '🔑', '🔔', '🔍', '🔧', '🧭', '🧩', '🧪', '🧵', '🧺', '🪴',
];

/** One emoji of the list. `random` is a number from 0 up to, not including, 1. */
export function randomEmoji(random: number = Math.random()): string {
  const at = Math.min(CARD_EMOJI.length - 1, Math.max(0, Math.floor(random * CARD_EMOJI.length)));
  return CARD_EMOJI[at] ?? '📌';
}

type Icon = Component<{ class?: string }>;

const TYPE_ICONS: Partial<Record<PropertyType, Icon>> = {
  text: TypeIcon,
  number: HashIcon,
  email: AtSignIcon,
  phone: PhoneIcon,
  url: LinkIcon,
  select: CircleDotIcon,
  multiSelect: TagsIcon,
  date: CalendarIcon,
  person: UserIcon,
  multiPerson: UsersIcon,
  checkbox: SquareCheckIcon,
  file: PaperclipIcon,
  createdTime: ClockIcon,
  updatedTime: ClockIcon,
  createdBy: UserPenIcon,
  updatedBy: UserPenIcon,
};

/** The picture for a kind of property; a kind this app does not know gets the one of a text. */
export function propertyTypeIcon(type: string): Icon {
  return (TYPE_ICONS as Record<string, Icon | undefined>)[type] ?? TypeIcon;
}
