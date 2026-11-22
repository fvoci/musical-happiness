/**
 * Changing the address of the reader's account. The account's password asks
 * for it; the link mailed to the new address makes it, and until then the
 * account keeps the address it has (`User.email`, with `User.pendingEmail`
 * the one it waits for). A server that sends no mail changes it at once.
 */
export interface EmailChange {
  /**
   * Asks for the account's address to become `email`; `password` is the
   * account's. A wrong one is 400 with the reason `invalid_credentials`, too
   * many tries 429, an account without a password 409 (`password_not_set`),
   * what is no address or the one the account has 400 at `/email`. The answer
   * is the same whether or not the address is somebody else's: no link is
   * mailed then. Only a server without mail says so (409, `email_taken`).
   * Asked again, it replaces what waited. The mail is in `language`.
   */
  request(email: string, password: string, language: string): Promise<void>;
  /** Mails the link again; the one before it works no more. 404 when nothing waits. */
  resend(language: string): Promise<void>;
  /** Takes the asking back: the link mailed changes nothing. */
  cancel(): Promise<void>;
  /**
   * Changes the address by the token of the link, signed in or not. A link
   * that changes nothing (used, replaced, taken back, ended, or its address
   * became somebody else's) is 400 at `/token`.
   */
  confirm(token: string): Promise<void>;
}
