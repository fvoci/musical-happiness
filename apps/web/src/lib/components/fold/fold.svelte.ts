/**
 * Whether a part of a panel that folds shows what it holds: as it stands by
 * itself (`byItself`, asked each time, so that it may follow what was read),
 * until the reader folds or unfolds it, which then holds.
 */
export class Fold {
  #byHand = $state<boolean | undefined>(undefined);
  readonly #byItself: () => boolean;

  constructor(byItself: () => boolean = () => false) {
    this.#byItself = byItself;
  }

  get open(): boolean {
    return this.#byHand ?? this.#byItself();
  }

  toggle(): void {
    this.#byHand = !this.open;
  }
}
