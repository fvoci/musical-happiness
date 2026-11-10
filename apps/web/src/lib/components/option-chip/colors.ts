/**
 * The colours a person can give a select option. An option stores the name
 * of its colour (`propColorBrown`); the fill it is drawn with is a token of
 * styles/app.css, and its text is always the page's text colour.
 */
import { m } from '$lib/i18n/paraglide/messages.js';

/** The colour names, in the order they are offered. */
export const OPTION_COLORS = [
  'propColorDefault',
  'propColorGray',
  'propColorBrown',
  'propColorOrange',
  'propColorYellow',
  'propColorGreen',
  'propColorBlue',
  'propColorPurple',
  'propColorPink',
  'propColorRed',
] as const;

export type OptionColor = (typeof OPTION_COLORS)[number];

/** The colour a new option starts with. */
export const DEFAULT_OPTION_COLOR: OptionColor = 'propColorDefault';

// Whole class names, so that the style sheet is built with them.
const FILLS: Record<OptionColor, string> = {
  propColorDefault: 'bg-option-default',
  propColorGray: 'bg-option-gray',
  propColorBrown: 'bg-option-brown',
  propColorOrange: 'bg-option-orange',
  propColorYellow: 'bg-option-yellow',
  propColorGreen: 'bg-option-green',
  propColorBlue: 'bg-option-blue',
  propColorPurple: 'bg-option-purple',
  propColorPink: 'bg-option-pink',
  propColorRed: 'bg-option-red',
};

const NAMES: Record<OptionColor, () => string> = {
  propColorDefault: m.property_color_default,
  propColorGray: m.property_color_gray,
  propColorBrown: m.property_color_brown,
  propColorOrange: m.property_color_orange,
  propColorYellow: m.property_color_yellow,
  propColorGreen: m.property_color_green,
  propColorBlue: m.property_color_blue,
  propColorPurple: m.property_color_purple,
  propColorPink: m.property_color_pink,
  propColorRed: m.property_color_red,
};

export function isOptionColor(color: string): color is OptionColor {
  return (OPTION_COLORS as readonly string[]).includes(color);
}

/** The class that fills a label with a colour; a name that is not a colour draws as the default. */
export function optionFill(color: string): string {
  return FILLS[isOptionColor(color) ? color : DEFAULT_OPTION_COLOR];
}

/** The name of a colour in the reader's language. */
export function optionColorName(color: OptionColor): string {
  return NAMES[color]();
}
