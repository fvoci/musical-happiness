-- The baseline schema of a PostgreSQL database.
--
-- Identifiers are version 7 UUIDs: `uuid` columns. Times are milliseconds since the Unix
-- epoch. The two baseline files describe the same tables; they differ in the column
-- types only.
--
-- A migration that has been released is never edited; a change is a new file with the
-- next number.

CREATE TABLE teams (
    id UUID NOT NULL PRIMARY KEY,
    -- A secret; whoever holds it may register.
    signup_token TEXT NOT NULL,
    settings JSON NOT NULL,
    modified_by UUID,
    create_at BIGINT NOT NULL,
    update_at BIGINT NOT NULL
);

CREATE TABLE users (
    id UUID NOT NULL PRIMARY KEY,
    username TEXT NOT NULL,
    email TEXT NOT NULL,
    password TEXT NOT NULL,
    create_at BIGINT NOT NULL,
    update_at BIGINT NOT NULL,
    -- Zero while the account is in use.
    delete_at BIGINT NOT NULL DEFAULT 0
);
-- A name and an address belong to one account, whatever their case. SQLite's lower()
-- folds ASCII letters only.
CREATE UNIQUE INDEX users_username_key ON users (lower(username));
CREATE UNIQUE INDEX users_email_key ON users (lower(email));

CREATE TABLE sessions (
    id UUID NOT NULL PRIMARY KEY,
    token TEXT NOT NULL,
    user_id UUID NOT NULL,
    auth_service TEXT NOT NULL,
    props JSON NOT NULL,
    create_at BIGINT NOT NULL,
    update_at BIGINT NOT NULL
);
CREATE UNIQUE INDEX sessions_token_key ON sessions (token);
CREATE INDEX sessions_user_id_idx ON sessions (user_id);

CREATE TABLE preferences (
    user_id UUID NOT NULL,
    category TEXT NOT NULL,
    name TEXT NOT NULL,
    value TEXT NOT NULL,
    PRIMARY KEY (user_id, category, name)
);

CREATE TABLE boards (
    id UUID NOT NULL PRIMARY KEY,
    team_id UUID NOT NULL,
    -- NULL for a built-in template, which no user made.
    created_by UUID,
    modified_by UUID,
    type TEXT NOT NULL,
    minimum_role TEXT NOT NULL DEFAULT '',
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
    update_at BIGINT NOT NULL,
    delete_at BIGINT NOT NULL DEFAULT 0
);
CREATE INDEX boards_team_id_is_template_idx ON boards (team_id, is_template);

CREATE TABLE boards_history (
    id UUID NOT NULL,
    insert_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT now(),
    team_id UUID NOT NULL,
    created_by UUID,
    modified_by UUID,
    type TEXT NOT NULL,
    minimum_role TEXT NOT NULL DEFAULT '',
    title TEXT NOT NULL,
    description TEXT NOT NULL,
    icon TEXT NOT NULL,
    show_description BOOLEAN NOT NULL,
    is_template BOOLEAN NOT NULL,
    is_builtin BOOLEAN NOT NULL DEFAULT false,
    template_version BIGINT NOT NULL DEFAULT 0,
    properties JSONB NOT NULL,
    card_properties JSONB NOT NULL,
    create_at BIGINT NOT NULL,
    update_at BIGINT NOT NULL,
    delete_at BIGINT NOT NULL DEFAULT 0,
    PRIMARY KEY (id, insert_at)
);

CREATE TABLE board_members (
    board_id UUID NOT NULL,
    user_id UUID NOT NULL,
    scheme_admin BOOLEAN NOT NULL,
    scheme_editor BOOLEAN NOT NULL,
    scheme_commenter BOOLEAN NOT NULL,
    scheme_viewer BOOLEAN NOT NULL,
    PRIMARY KEY (board_id, user_id)
);
CREATE INDEX board_members_user_id_idx ON board_members (user_id);

CREATE TABLE blocks (
    id UUID NOT NULL PRIMARY KEY,
    insert_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT now(),
    board_id UUID NOT NULL,
    parent_id UUID,
    created_by UUID,
    modified_by UUID,
    "schema" BIGINT NOT NULL,
    type TEXT NOT NULL,
    title TEXT NOT NULL,
    fields JSON NOT NULL,
    create_at BIGINT NOT NULL,
    update_at BIGINT NOT NULL,
    delete_at BIGINT NOT NULL DEFAULT 0
);
CREATE INDEX blocks_board_id_parent_id_idx ON blocks (board_id, parent_id);

CREATE TABLE blocks_history (
    id UUID NOT NULL,
    insert_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT now(),
    board_id UUID NOT NULL,
    parent_id UUID,
    created_by UUID,
    modified_by UUID,
    "schema" BIGINT NOT NULL,
    type TEXT NOT NULL,
    title TEXT NOT NULL,
    fields JSON NOT NULL,
    create_at BIGINT NOT NULL,
    update_at BIGINT NOT NULL,
    delete_at BIGINT NOT NULL DEFAULT 0,
    PRIMARY KEY (id, insert_at)
);
CREATE INDEX blocks_history_board_id_idx ON blocks_history (board_id);

CREATE TABLE categories (
    id UUID NOT NULL PRIMARY KEY,
    name TEXT NOT NULL,
    user_id UUID NOT NULL,
    team_id UUID NOT NULL,
    create_at BIGINT NOT NULL,
    update_at BIGINT NOT NULL,
    delete_at BIGINT NOT NULL DEFAULT 0,
    collapsed BOOLEAN NOT NULL DEFAULT false,
    type TEXT NOT NULL,
    sort_order BIGINT NOT NULL DEFAULT 0
);
CREATE INDEX categories_user_id_team_id_idx ON categories (user_id, team_id);

CREATE TABLE category_boards (
    id UUID NOT NULL PRIMARY KEY,
    user_id UUID NOT NULL,
    category_id UUID NOT NULL,
    board_id UUID NOT NULL,
    create_at BIGINT NOT NULL,
    update_at BIGINT NOT NULL,
    sort_order BIGINT NOT NULL DEFAULT 0,
    hidden BOOLEAN NOT NULL DEFAULT false,
    -- A user files a board under one category.
    UNIQUE (user_id, board_id)
);
CREATE INDEX category_boards_category_id_idx ON category_boards (category_id);

CREATE TABLE file_info (
    id UUID NOT NULL PRIMARY KEY,
    -- A file is one board's.
    board_id UUID NOT NULL,
    -- The name the file is stored and asked for under: random, with the extension.
    name TEXT NOT NULL,
    -- The name it was uploaded with.
    original_name TEXT NOT NULL,
    extension TEXT NOT NULL,
    size BIGINT NOT NULL,
    -- Where the file lives in the file storage.
    path TEXT NOT NULL,
    archived BOOLEAN NOT NULL DEFAULT false,
    create_at BIGINT NOT NULL,
    delete_at BIGINT NOT NULL DEFAULT 0,
    UNIQUE (board_id, name)
);

CREATE TABLE sharing (
    -- The identifier of the shared board.
    id UUID NOT NULL PRIMARY KEY,
    enabled BOOLEAN NOT NULL,
    token TEXT NOT NULL,
    modified_by UUID,
    update_at BIGINT NOT NULL
);

CREATE TABLE subscriptions (
    block_type TEXT NOT NULL,
    block_id UUID NOT NULL,
    subscriber_type TEXT NOT NULL,
    subscriber_id UUID NOT NULL,
    notified_at BIGINT NOT NULL DEFAULT 0,
    create_at BIGINT NOT NULL,
    delete_at BIGINT NOT NULL DEFAULT 0,
    PRIMARY KEY (block_id, subscriber_id)
);
CREATE INDEX subscriptions_subscriber_id_idx ON subscriptions (subscriber_id);
