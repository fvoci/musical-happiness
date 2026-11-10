/**
 * Undo and redo of the changes a person makes.
 *
 * A change is a pair of functions: `redo` performs it and `undo` takes it
 * back. Whatever `redo` returns is handed to `undo`, so a change that creates
 * something (and learns its identifier only from the server's answer) can
 * remove exactly that. Changes that share a `groupId` are undone and redone
 * together.
 */

interface UndoCommand {
  checkpoint: number;
  undo: (value?: unknown) => Promise<void>;
  redo: () => Promise<unknown>;
  description: string | undefined;
  groupId: string | undefined;
  value: unknown;
}

export class UndoManager {
  /** Called whenever what can be undone or redone has changed. */
  onStateDidChange?: () => void;

  #commands: UndoCommand[] = [];
  #index = -1;
  #isExecuting = false;

  /** When the change that would be undone next was made; 0 without one. */
  get currentCheckpoint(): number {
    return this.#commands[this.#index]?.checkpoint ?? 0;
  }

  get undoDescription(): string | undefined {
    return this.#commands[this.#index]?.description;
  }

  get redoDescription(): string | undefined {
    return this.#commands[this.#index + 1]?.description;
  }

  get canUndo(): boolean {
    return this.#index !== -1;
  }

  get canRedo(): boolean {
    return this.#index < this.#commands.length - 1;
  }

  /** Performs a change and remembers how to take it back. Returns what `redo` returned. */
  async perform<T>(
    redo: () => Promise<T>,
    undo: (value: T) => Promise<void>,
    description?: string,
    groupId?: string,
    isDiscardable = false,
  ): Promise<T> {
    const value = await redo();
    this.registerUndo(
      { undo: undo as (value?: unknown) => Promise<void>, redo },
      description,
      groupId,
      value,
      isDiscardable,
    );
    return value;
  }

  /**
   * Remembers a change that has already been performed. A discardable change
   * takes the checkpoint of the change before it instead of the current time.
   */
  registerUndo(
    command: { undo: (value?: unknown) => Promise<void>; redo: () => Promise<unknown> },
    description?: string,
    groupId?: string,
    value?: unknown,
    isDiscardable = false,
  ): this {
    // A change made while undoing or redoing is part of that, not a new step.
    if (this.#isExecuting) return this;

    // Whatever was undone and not redone is gone once something new happens.
    this.#commands.splice(this.#index + 1);

    const previous = this.#commands.at(-1);
    const checkpoint = isDiscardable
      ? this.#commands.length > 1 && previous
        ? previous.checkpoint
        : 0
      : Date.now();
    this.#commands.push({
      checkpoint,
      undo: command.undo,
      redo: command.redo,
      description,
      groupId,
      value,
    });
    this.#index = this.#commands.length - 1;
    this.onStateDidChange?.();
    return this;
  }

  async undo(): Promise<this> {
    if (this.#isExecuting) return this;
    let command = this.#commands[this.#index];
    if (!command) return this;

    const groupId = command.groupId;
    do {
      await this.#execute(command, 'undo');
      this.#index -= 1;
      command = this.#commands[this.#index];
    } while (groupId && command?.groupId === groupId);

    this.onStateDidChange?.();
    return this;
  }

  async redo(): Promise<this> {
    if (this.#isExecuting) return this;
    let command = this.#commands[this.#index + 1];
    if (!command) return this;

    const groupId = command.groupId;
    do {
      await this.#execute(command, 'redo');
      this.#index += 1;
      command = this.#commands[this.#index + 1];
    } while (groupId && command?.groupId === groupId);

    this.onStateDidChange?.();
    return this;
  }

  clear(): void {
    const hadCommands = this.#commands.length > 0;
    this.#commands = [];
    this.#index = -1;
    if (hadCommands) this.onStateDidChange?.();
  }

  async #execute(command: UndoCommand, action: 'undo' | 'redo'): Promise<void> {
    this.#isExecuting = true;
    try {
      if (action === 'redo') command.value = await command.redo();
      else await command.undo(command.value);
    } finally {
      this.#isExecuting = false;
    }
  }
}
