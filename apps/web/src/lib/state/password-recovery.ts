/** Password recovery without a signed-in account. */
export interface PasswordRecovery {
  /** Consumes a one-hour link and ends all of the account's sessions. */
  complete(token: string, password: string): Promise<void>;
}
