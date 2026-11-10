-- The baseline schema of a PostgreSQL database.
--
-- Identifiers are version 7 UUIDs: `uuid` columns. Times are milliseconds since the Unix
-- epoch. The two baseline files describe the same tables; they differ in the column
-- types only.
--
-- A migration that has been released is never edited; a change is a new file with the
-- next number.

CREATE TABLE users (
    id UUID NOT NULL PRIMARY KEY,
    username TEXT NOT NULL,
    email TEXT NOT NULL,
    -- An Argon2id PHC string.
    password_hash TEXT NOT NULL,
    create_at BIGINT NOT NULL,
    update_at BIGINT NOT NULL,
    -- Zero while the account is in use. A user is never removed: what they made keeps
    -- pointing at them.
    delete_at BIGINT NOT NULL DEFAULT 0,
    -- An administrator of the instance: sees and renews the signup token, makes others
    -- administrators.
    is_admin BOOLEAN NOT NULL DEFAULT FALSE
);
-- A name and an address belong to one account, whatever their case. SQLite's lower()
-- folds ASCII letters only.
CREATE UNIQUE INDEX users_username_key ON users (lower(username));
CREATE UNIQUE INDEX users_email_key ON users (lower(email));

-- What is true of the installation as a whole: one row.
CREATE TABLE instance (
    id SMALLINT NOT NULL PRIMARY KEY CHECK (id = 1),
    -- Whether the first user has registered. Whoever turns this on is that user, and the
    -- first administrator; nobody else gets in without the signup token.
    bootstrapped BOOLEAN NOT NULL DEFAULT FALSE
);
INSERT INTO instance (id, bootstrapped) VALUES (1, FALSE);

CREATE TABLE teams (
    id UUID NOT NULL PRIMARY KEY,
    -- A secret; whoever holds it may register.
    signup_token TEXT NOT NULL,
    settings JSON NOT NULL,
    modified_by UUID REFERENCES users (id) ON DELETE SET NULL,
    create_at BIGINT NOT NULL,
    update_at BIGINT NOT NULL
);

CREATE TABLE sessions (
    id UUID NOT NULL PRIMARY KEY,
    -- The SHA-256 digest of the session token, in hexadecimal. The token itself is not
    -- stored: what is here does not sign anyone in.
    token_hash TEXT NOT NULL,
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    created_at BIGINT NOT NULL,
    -- Written when the session is used, but not on every request.
    last_used_at BIGINT NOT NULL,
    -- When the session ends whatever its use.
    expires_at BIGINT NOT NULL,
    user_agent TEXT NOT NULL
);
CREATE UNIQUE INDEX sessions_token_hash_key ON sessions (token_hash);
CREATE INDEX sessions_user_id_idx ON sessions (user_id);

CREATE TABLE preferences (
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    category TEXT NOT NULL,
    name TEXT NOT NULL,
    value TEXT NOT NULL,
    PRIMARY KEY (user_id, category, name)
);

-- The boards that exist. A deleted board is not here: it is in deleted_boards until it
-- is restored or its time there is up.
CREATE TABLE boards (
    id UUID NOT NULL PRIMARY KEY,
    team_id UUID NOT NULL REFERENCES teams (id),
    -- NULL for a built-in template, which no user made. The row of whoever made a board
    -- stays.
    created_by UUID REFERENCES users (id),
    modified_by UUID REFERENCES users (id),
    type TEXT NOT NULL,
    -- The role whoever joins the board, or is added by somebody who does not manage its
    -- roles, gets. Never empty, and never admin: nobody becomes an administrator by
    -- joining.
    minimum_role TEXT NOT NULL DEFAULT 'editor'
        CHECK (minimum_role IN ('viewer', 'commenter', 'editor')),
    title TEXT NOT NULL,
    description TEXT NOT NULL,
    icon TEXT NOT NULL,
    show_description BOOLEAN NOT NULL,
    is_template BOOLEAN NOT NULL,
    -- A template the server ships with: offered to every team, changed by nobody.
    is_builtin BOOLEAN NOT NULL DEFAULT false,
    template_version BIGINT NOT NULL DEFAULT 0,
    properties JSONB NOT NULL,
    card_properties JSONB NOT NULL,
    create_at BIGINT NOT NULL,
    update_at BIGINT NOT NULL
);
CREATE INDEX boards_team_id_is_template_idx ON boards (team_id, is_template);

-- A membership is nothing without its board and its user.
CREATE TABLE board_members (
    board_id UUID NOT NULL REFERENCES boards (id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    scheme_admin BOOLEAN NOT NULL,
    scheme_editor BOOLEAN NOT NULL,
    scheme_commenter BOOLEAN NOT NULL,
    scheme_viewer BOOLEAN NOT NULL,
    PRIMARY KEY (board_id, user_id)
);
CREATE INDEX board_members_user_id_idx ON board_members (user_id);

-- The blocks that exist. A deleted block is in deleted_blocks.
--
-- A block is its board's and, below the top, its parent's. Neither reference lets the
-- row it points at go: a board or a parent that is deleted would otherwise take its
-- blocks along past the trash. The store moves them there first, and the database
-- refuses if it forgot. The parent is a block of the same board.
CREATE TABLE blocks (
    id UUID NOT NULL PRIMARY KEY,
    board_id UUID NOT NULL REFERENCES boards (id),
    -- NULL for a block directly on its board.
    parent_id UUID,
    created_by UUID REFERENCES users (id),
    modified_by UUID REFERENCES users (id),
    "schema" BIGINT NOT NULL,
    type TEXT NOT NULL,
    title TEXT NOT NULL,
    fields JSON NOT NULL,
    create_at BIGINT NOT NULL,
    update_at BIGINT NOT NULL,
    UNIQUE (board_id, id),
    FOREIGN KEY (board_id, parent_id) REFERENCES blocks (board_id, id)
);
CREATE INDEX blocks_board_id_parent_id_idx ON blocks (board_id, parent_id);
CREATE INDEX blocks_parent_id_idx ON blocks (parent_id);

-- The trash. What is deleted is moved here whole, as JSON, and comes back from here.
-- Nothing here is referred to by a key and nothing here refers to anything by one: a
-- row describes what was, including users and boards that may be gone since.
CREATE TABLE deleted_boards (
    id UUID NOT NULL PRIMARY KEY,
    team_id UUID NOT NULL,
    deleted_at BIGINT NOT NULL,
    deleted_by UUID,
    -- The deletion the board's blocks in deleted_blocks went with.
    deletion_id UUID NOT NULL,
    board JSON NOT NULL,
    -- The memberships and the file records at the time, which go when the board goes.
    members JSON NOT NULL,
    files JSON NOT NULL
);
CREATE INDEX deleted_boards_deleted_at_idx ON deleted_boards (deleted_at);

CREATE TABLE deleted_blocks (
    id UUID NOT NULL PRIMARY KEY,
    board_id UUID NOT NULL,
    parent_id UUID,
    deleted_at BIGINT NOT NULL,
    deleted_by UUID,
    -- Blocks that went with one deletion (a card with what is on it, a board with all
    -- its blocks) share this and come back together.
    deletion_id UUID NOT NULL,
    block JSON NOT NULL
);
CREATE INDEX deleted_blocks_deletion_id_idx ON deleted_blocks (deletion_id);
CREATE INDEX deleted_blocks_board_id_idx ON deleted_blocks (board_id);
CREATE INDEX deleted_blocks_deleted_at_idx ON deleted_blocks (deleted_at);

CREATE TABLE categories (
    id UUID NOT NULL PRIMARY KEY,
    name TEXT NOT NULL,
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    team_id UUID NOT NULL REFERENCES teams (id),
    create_at BIGINT NOT NULL,
    update_at BIGINT NOT NULL,
    collapsed BOOLEAN NOT NULL DEFAULT false,
    type TEXT NOT NULL,
    sort_order BIGINT NOT NULL DEFAULT 0
);
CREATE INDEX categories_user_id_team_id_idx ON categories (user_id, team_id);

CREATE TABLE category_boards (
    id UUID NOT NULL PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    category_id UUID NOT NULL REFERENCES categories (id) ON DELETE CASCADE,
    board_id UUID NOT NULL REFERENCES boards (id) ON DELETE CASCADE,
    create_at BIGINT NOT NULL,
    update_at BIGINT NOT NULL,
    sort_order BIGINT NOT NULL DEFAULT 0,
    hidden BOOLEAN NOT NULL DEFAULT false,
    -- A user files a board under one category.
    UNIQUE (user_id, board_id)
);
CREATE INDEX category_boards_category_id_idx ON category_boards (category_id);
CREATE INDEX category_boards_board_id_idx ON category_boards (board_id);

CREATE TABLE file_info (
    id UUID NOT NULL PRIMARY KEY,
    -- A file is one board's. Its record goes with the board; while the board is in the
    -- trash the record is kept there, with the board.
    board_id UUID NOT NULL REFERENCES boards (id) ON DELETE CASCADE,
    -- The name the file is stored and asked for under: random, with the extension.
    name TEXT NOT NULL,
    -- The name it was uploaded with.
    original_name TEXT NOT NULL,
    extension TEXT NOT NULL,
    size BIGINT NOT NULL,
    -- Where the file lives in the file storage.
    path TEXT NOT NULL,
    create_at BIGINT NOT NULL,
    UNIQUE (board_id, name)
);

CREATE TABLE sharing (
    board_id UUID NOT NULL PRIMARY KEY REFERENCES boards (id) ON DELETE CASCADE,
    enabled BOOLEAN NOT NULL,
    -- The read token, which the server makes: a secret that opens one board. Absent
    -- while the board is not shared; turning sharing off forgets the token.
    token TEXT UNIQUE,
    modified_by UUID REFERENCES users (id) ON DELETE SET NULL,
    update_at BIGINT NOT NULL
);

-- A subscription ends with its block and with its subscriber.
CREATE TABLE subscriptions (
    block_type TEXT NOT NULL,
    block_id UUID NOT NULL REFERENCES blocks (id) ON DELETE CASCADE,
    subscriber_type TEXT NOT NULL,
    subscriber_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    notified_at BIGINT NOT NULL DEFAULT 0,
    create_at BIGINT NOT NULL,
    PRIMARY KEY (block_id, subscriber_id)
);
CREATE INDEX subscriptions_subscriber_id_idx ON subscriptions (subscriber_id);
