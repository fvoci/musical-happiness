/**
 * Reactions: the backend in memory keeps the server's rules
 * (`crates/server/src/api/reactions.rs`), and the page shown keeps them
 * apart from its comments (`CommentThreadStore`): the reader's toggle shows
 * at once and goes back when refused, and what somebody else does comes as
 * the block's reactions as they are now.
 */
import { describe, expect, it } from 'vitest';

import type { MemberRole } from '$lib/logic/wire.ts';
import { BackendError, WikiStore, type Change } from '$lib/state/index.ts';
import { withMine, type Reaction } from '$lib/state/reaction-types.ts';

import { settle, setup, text } from './support.ts';

const rejection = async (promise: Promise<unknown>): Promise<BackendError> => {
  try {
    await promise;
  } catch (error) {
    if (error instanceof BackendError) return error;
    throw error;
  }
  throw new Error('the call did not reject');
};

/** A wiki of the reader with a page and a comment on it, the other person on it with `role`. */
async function aPage(role: Exclude<MemberRole, '' | 'admin'> = 'commenter') {
  const made = setup();
  const workspaceId = made.world.workspace.id;
  const wiki = await made.backend.wiki.createWiki(workspaceId, {
    title: 'Handbook',
    visibility: 'open',
  });
  await made.backend.patchContainer(wiki.id, { minimumRole: role });
  await made.other.joinContainer(wiki.id);
  const page = await made.backend.wiki.createPage(wiki.id, { title: 'Page' });
  const comment = await made.backend.wiki.createComment(wiki.id, page.id, text('hello'));
  return { ...made, workspaceId, wikiId: wiki.id, pageId: page.id, commentId: comment.id };
}

describe('the reactions of a reader', () => {
  const [ann, bob] = ['ann', 'bob'];

  it('go on last and come off, the reader last among those who reacted', () => {
    const none: Reaction[] = [];
    const one = withMine(none, '👍', ann, true);
    expect(one).toEqual([{ emoji: '👍', userIds: [ann] }]);
    const two = withMine(withMine(one, '🎉', bob, true), '👍', bob, true);
    expect(two).toEqual([
      { emoji: '👍', userIds: [ann, bob] },
      { emoji: '🎉', userIds: [bob] },
    ]);
    expect(withMine(two, '👍', ann, true)).toBe(two);
    expect(withMine(withMine(two, '🎉', bob, false), '👍', ann, false)).toEqual([
      { emoji: '👍', userIds: [bob] },
    ]);
    expect(withMine(none, '👀', ann, false)).toBe(none);
  });
});

describe('reactions in memory', () => {
  it('are added once by who may comment, read with the comments and the page, and told', async () => {
    const made = await aPage('commenter');
    const told: Change[] = [];
    made.backend.watchContainer(made.wikiId, (change) => told.push(change));
    const { reactions } = made.other;
    const at = [made.workspaceId, made.wikiId] as const;
    await reactions.add(...at, made.commentId, '👍');
    await reactions.add(...at, made.commentId, '👍');
    await reactions.add(...at, made.pageId, '🎉');
    const [comment] = await made.backend.wiki.comments(made.wikiId, made.pageId);
    expect(comment?.reactions).toEqual([{ emoji: '👍', userIds: [made.other.userId] }]);
    const page = await made.backend.wiki.page(made.wikiId, made.pageId);
    expect(page.reactions).toEqual([{ emoji: '🎉', userIds: [made.other.userId] }]);
    await reactions.remove(...at, made.commentId, '👍');
    await reactions.remove(...at, made.commentId, '👍');
    const [again] = await made.backend.wiki.comments(made.wikiId, made.pageId);
    expect(again?.reactions).toBeUndefined();
    await settle();
    expect(told.map((change) => change.kind)).toEqual(['reactions', 'reactions', 'reactions']);
  });

  it('are refused to a viewer, past twenty emoji, and on what is not there', async () => {
    const made = await aPage('viewer');
    const at = [made.workspaceId, made.wikiId] as const;
    const theirs = made.other.reactions;
    const mine = made.backend.reactions;
    expect((await rejection(theirs.add(...at, made.commentId, '👍'))).status).toBe(403);
    const animals = Array.from({ length: 21 }, (_, at) => String.fromCodePoint(0x1f400 + at));
    for (const animal of animals.slice(0, 20)) await mine.add(...at, made.pageId, animal);
    const refused = await rejection(mine.add(...at, made.pageId, animals[20] ?? ''));
    expect([refused.status, refused.reason]).toEqual([400, 'too_many_reactions']);
    expect((await rejection(mine.add(...at, 'nothing', '👍'))).status).toBe(404);
  });
});

describe('the reactions of the page shown', () => {
  it('show the reader’s toggle at once, keep it, and take it back when refused', async () => {
    const made = await aPage();
    await made.workspace.load();
    const store = new WikiStore(made.workspace, made.wikiId);
    await store.load();
    const comments = store.showPage(made.pageId)?.comments;
    await settle();
    if (!comments) throw new Error('no page shown');
    expect(comments.canReact).toBe(true);
    const me = made.backend.userId;

    made.backend.latency = 20;
    const adding = comments.toggleReaction(made.commentId, '🚀');
    expect(comments.reactionsOf(made.commentId)).toEqual([{ emoji: '🚀', userIds: [me] }]);
    await adding;
    await settle();
    expect(comments.reactionsOf(made.commentId)).toEqual([{ emoji: '🚀', userIds: [me] }]);

    made.backend.latency = 0;
    made.backend.failNext('reactions', new BackendError(500, { detail: 'down' }));
    const refused = comments.toggleReaction(made.commentId, '🚀');
    expect(comments.reactionsOf(made.commentId)).toEqual([]);
    await refused;
    expect(comments.reactionsOf(made.commentId)).toEqual([{ emoji: '🚀', userIds: [me] }]);
    store.close();
  });

  it('follow what somebody else does, on the page and on its comments', async () => {
    const made = await aPage();
    await made.workspace.load();
    const store = new WikiStore(made.workspace, made.wikiId);
    await store.load();
    const comments = store.showPage(made.pageId)?.comments;
    await settle();
    if (!comments) throw new Error('no page shown');
    const theirs = made.other.reactions;
    const at = [made.workspaceId, made.wikiId] as const;
    await theirs.add(...at, made.pageId, '👀');
    await theirs.add(...at, made.commentId, '❤️');
    await settle();
    expect(comments.reactionsOf(made.pageId)).toEqual([
      { emoji: '👀', userIds: [made.other.userId] },
    ]);
    expect(comments.reactionsOf(made.commentId)).toEqual([
      { emoji: '❤️', userIds: [made.other.userId] },
    ]);
    // A comment changed carries none, and keeps its own.
    const [comment] = await made.backend.wiki.comments(made.wikiId, made.pageId);
    if (!comment) throw new Error('no comment');
    await made.backend.wiki.patchComment(made.wikiId, made.pageId, comment.id, {
      body: text('hello again'),
      readAt: comment.updatedAt,
    });
    await settle();
    expect(comments.reactionsOf(made.commentId)).toHaveLength(1);
    await theirs.remove(...at, made.commentId, '❤️');
    await settle();
    expect(comments.reactionsOf(made.commentId)).toEqual([]);
    store.close();
  });

  it('are read on a page opened later', async () => {
    const made = await aPage();
    const theirs = made.other.reactions;
    await theirs.add(made.workspaceId, made.wikiId, made.pageId, '😄');
    await theirs.add(made.workspaceId, made.wikiId, made.commentId, '👍');
    await made.workspace.load();
    const store = new WikiStore(made.workspace, made.wikiId);
    await store.load();
    const comments = store.showPage(made.pageId)?.comments;
    await settle();
    if (!comments) throw new Error('no page shown');
    expect(comments.reactionsOf(made.pageId).map((one) => one.emoji)).toEqual(['😄']);
    expect(comments.reactionsOf(made.commentId).map((one) => one.emoji)).toEqual(['👍']);
    expect(comments.comments[0]).not.toHaveProperty('reactions');
    store.close();
  });
});
