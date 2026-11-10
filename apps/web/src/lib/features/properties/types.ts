/**
 * What the property components are given. They take values and callbacks
 * only: nothing here reads a store or talks to a server.
 */
import type { Presentation } from '$lib/components/picker-surface/presentation.ts';
import type { Regional } from '$lib/logic/regional.ts';
import type { UserNames } from '$lib/logic/users.ts';
import type { Card, PropertyOption, PropertyTemplate, PropertyValue } from '$lib/logic/wire.ts';

/** A person who can be the value of a person property. */
export interface PropertyUser extends UserNames {
  id: string;
}

/**
 * Changes to the options of a select property. The component only reports
 * them; changing the property's definition is up to the caller.
 */
export interface OptionCallbacks {
  /**
   * A person typed a name that no option has and asked for it. The caller
   * makes the option (its identifier and colour are the caller's to choose)
   * and returns it; the component then puts it into the value.
   */
  onCreateOption?: (name: string) => PropertyOption | Promise<PropertyOption>;
  onRenameOption?: (option: PropertyOption, name: string) => void;
  /** `color` is a colour name such as `propColorBrown`. */
  onRecolorOption?: (option: PropertyOption, color: string) => void;
  onDeleteOption?: (option: PropertyOption) => void;
}

/** The reader: their language, their regional settings, and the current moment. */
export interface ReaderContext {
  /** BCP 47 tag of the language the app is shown in. */
  locale: string;
  /** Time zone, first day of the week and way of writing hours, from the account. */
  regional: Regional;
  /** The current moment; defaults to the clock. Decides what "today" is. */
  now?: number;
}

export interface PropertyValueProps extends OptionCallbacks, ReaderContext {
  template: PropertyTemplate;
  /** The card the value belongs to; created and updated times and people come from it. */
  card: Card;
  /** The stored value of the property on the card, if any. */
  value: PropertyValue | undefined;
  /**
   * `display` only shows the value (a table cell that is not being edited,
   * a badge on a kanban card); `edit` lets a person change it.
   */
  mode?: 'display' | 'edit';
  /** The reader may not change the value. */
  readOnly?: boolean;
  /** Show a word such as "Empty" where there is no value. */
  showEmptyPlaceholder?: boolean;
  /** Start in the editing state: the field focused, or the picker open. */
  startOpen?: boolean;
  /** The people a person property can be set to. */
  users?: readonly PropertyUser[];
  /** How names are shown: one of the formats of logic/users.ts. */
  nameFormat?: string;
  /** How pickers open. The caller decides; nothing here looks at the viewport. */
  presentation?: Presentation;
  /**
   * The new value to store. A cleared value is `''` (or `[]` for the kinds
   * that hold a list); a cleared date is `undefined`. A date is the wire
   * value that `formatDateValue` makes.
   */
  onChange?: (value: PropertyValue | undefined) => void;
  /** Puts a text on the clipboard; defaults to the browser's clipboard. */
  copyText?: (text: string) => void | Promise<void>;
}
