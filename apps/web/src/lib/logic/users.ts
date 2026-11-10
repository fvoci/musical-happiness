/** How a person's name is shown. */

export interface UserNames {
  username: string;
  nickname: string;
  firstname: string;
  lastname: string;
}

export const SHOW_USERNAME = 'username';
export const SHOW_NICKNAME_FULL_NAME = 'nickname_full_name';
export const SHOW_FULL_NAME = 'full_name';

/** First and last name, or whichever of the two there is. */
export function fullName(user: UserNames): string {
  return [user.firstname, user.lastname].filter((part) => part !== '').join(' ');
}

/**
 * The name to show for a user.
 *
 * `serverFormat` is the format the server is configured with and
 * `ownFormat` the one the reader chose, which wins when set. With neither,
 * or when the chosen name is empty, it is the username.
 */
export function userDisplayName(user: UserNames, serverFormat: string, ownFormat = ''): string {
  const format = ownFormat || serverFormat;
  if (format === SHOW_NICKNAME_FULL_NAME) {
    return user.nickname || fullName(user) || user.username;
  }
  if (format === SHOW_FULL_NAME) {
    return fullName(user) || user.username;
  }
  return user.username;
}
