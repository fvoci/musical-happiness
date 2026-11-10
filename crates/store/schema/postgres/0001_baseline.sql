-- The schema of a new PostgreSQL database: what the original server's 40 migrations
-- leave behind, with the same column types, constraints and indexes. `{p}` is the table
-- prefix.
--
-- Unused columns (root_id, channel_id, workspace_id, ...) and the notification_hints
-- table are kept so that the original server can open the database too.

CREATE TABLE {p}blocks (
    id VARCHAR(36) NOT NULL,
    insert_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT now(),
    parent_id VARCHAR(36),
    schema BIGINT,
    type TEXT,
    title TEXT,
    fields JSON,
    create_at BIGINT,
    update_at BIGINT,
    delete_at BIGINT,
    root_id VARCHAR(36),
    modified_by VARCHAR(36) NOT NULL,
    channel_id VARCHAR(36) NOT NULL,
    created_by VARCHAR(36) NOT NULL,
    board_id VARCHAR(36),
    CONSTRAINT {p}blocks_pkey1 PRIMARY KEY (id)
);
CREATE INDEX idx_blocks_board_id_parent_id ON {p}blocks (board_id, parent_id);

CREATE TABLE {p}blocks_history (
    id VARCHAR(36) NOT NULL,
    insert_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT now(),
    parent_id VARCHAR(36),
    schema BIGINT,
    type TEXT,
    title TEXT,
    fields JSON,
    create_at BIGINT,
    update_at BIGINT,
    delete_at BIGINT,
    root_id VARCHAR(36),
    modified_by VARCHAR(36),
    channel_id VARCHAR(36),
    created_by VARCHAR(36),
    board_id VARCHAR(36),
    CONSTRAINT {p}blocks_pkey PRIMARY KEY (id, insert_at)
);

CREATE TABLE {p}boards (
    id VARCHAR(36) NOT NULL,
    insert_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT now(),
    team_id VARCHAR(36) NOT NULL,
    channel_id VARCHAR(36),
    created_by VARCHAR(36),
    modified_by VARCHAR(36),
    type VARCHAR(1) NOT NULL,
    title TEXT NOT NULL,
    description TEXT,
    icon VARCHAR(256),
    show_description BOOLEAN,
    is_template BOOLEAN,
    template_version INTEGER DEFAULT 0,
    properties JSONB,
    card_properties JSONB,
    create_at BIGINT,
    update_at BIGINT,
    delete_at BIGINT,
    minimum_role VARCHAR(36) NOT NULL DEFAULT '',
    CONSTRAINT {p}boards_pkey PRIMARY KEY (id)
);
CREATE INDEX idx_boards_team_id_is_template ON {p}boards (team_id, is_template);
CREATE INDEX idx_boards_channel_id ON {p}boards (channel_id);

CREATE TABLE {p}boards_history (
    id VARCHAR(36) NOT NULL,
    insert_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT now(),
    team_id VARCHAR(36) NOT NULL,
    channel_id VARCHAR(36),
    created_by VARCHAR(36),
    modified_by VARCHAR(36),
    type VARCHAR(1) NOT NULL,
    title TEXT NOT NULL,
    description TEXT,
    icon VARCHAR(256),
    show_description BOOLEAN,
    is_template BOOLEAN,
    template_version INTEGER DEFAULT 0,
    properties JSONB,
    card_properties JSONB,
    create_at BIGINT,
    update_at BIGINT,
    delete_at BIGINT,
    minimum_role VARCHAR(36) NOT NULL DEFAULT '',
    CONSTRAINT {p}boards_history_pkey PRIMARY KEY (id, insert_at)
);

CREATE TABLE {p}board_members (
    board_id VARCHAR(36) NOT NULL,
    user_id VARCHAR(36) NOT NULL,
    roles VARCHAR(64),
    scheme_admin BOOLEAN,
    scheme_editor BOOLEAN,
    scheme_commenter BOOLEAN,
    scheme_viewer BOOLEAN,
    CONSTRAINT {p}board_members_pkey PRIMARY KEY (board_id, user_id)
);
CREATE INDEX idx_board_members_user_id ON {p}board_members (user_id);

CREATE TABLE {p}board_members_history (
    board_id VARCHAR(36) NOT NULL,
    user_id VARCHAR(36) NOT NULL,
    action VARCHAR(10),
    insert_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT now(),
    CONSTRAINT {p}board_members_history_pkey PRIMARY KEY (board_id, user_id, insert_at)
);
CREATE INDEX idx_board_members_history_user_id ON {p}board_members_history (user_id);
CREATE INDEX idx_board_members_history_board_id_user_id ON {p}board_members_history (board_id, user_id);

CREATE TABLE {p}categories (
    id VARCHAR(36) NOT NULL,
    name VARCHAR(100) NOT NULL,
    user_id VARCHAR(36) NOT NULL,
    team_id VARCHAR(36) NOT NULL,
    channel_id VARCHAR(36),
    create_at BIGINT,
    update_at BIGINT,
    delete_at BIGINT,
    collapsed BOOLEAN DEFAULT false,
    type VARCHAR(64),
    sort_order BIGINT,
    CONSTRAINT {p}categories_pkey PRIMARY KEY (id)
);
CREATE INDEX idx_categories_user_id_team_id ON {p}categories (user_id, team_id);

CREATE TABLE {p}category_boards (
    id VARCHAR(36) NOT NULL,
    user_id VARCHAR(36) NOT NULL,
    category_id VARCHAR(36) NOT NULL,
    board_id VARCHAR(36) NOT NULL,
    create_at BIGINT,
    update_at BIGINT,
    sort_order BIGINT,
    hidden BOOLEAN,
    CONSTRAINT {p}category_boards_pkey PRIMARY KEY (id),
    CONSTRAINT unique_user_category_board UNIQUE (user_id, board_id)
);
CREATE INDEX idx_category_boards_category_id ON {p}category_boards (category_id);

CREATE TABLE {p}file_info (
    id VARCHAR(26) NOT NULL,
    create_at BIGINT NOT NULL,
    delete_at BIGINT,
    name TEXT NOT NULL,
    extension VARCHAR(50) NOT NULL,
    size BIGINT NOT NULL,
    archived BOOLEAN,
    path VARCHAR(512)
);

CREATE TABLE {p}notification_hints (
    block_type VARCHAR(10),
    block_id VARCHAR(36) NOT NULL,
    workspace_id VARCHAR(36),
    modified_by_id VARCHAR(36),
    create_at BIGINT,
    notify_at BIGINT,
    CONSTRAINT {p}notification_hints_pkey PRIMARY KEY (block_id)
);

CREATE TABLE {p}preferences (
    userid VARCHAR(36) NOT NULL,
    category VARCHAR(32) NOT NULL,
    name VARCHAR(32) NOT NULL,
    value TEXT,
    CONSTRAINT {p}preferences_pkey PRIMARY KEY (userid, category, name)
);
CREATE INDEX idx_preferences_category ON {p}preferences (category);
CREATE INDEX idx_preferences_name ON {p}preferences (name);

CREATE TABLE {p}sessions (
    id VARCHAR(100) NOT NULL,
    token VARCHAR(100),
    user_id VARCHAR(100),
    props JSON,
    create_at BIGINT,
    update_at BIGINT,
    auth_service VARCHAR(20),
    CONSTRAINT {p}sessions_pkey PRIMARY KEY (id)
);

CREATE TABLE {p}sharing (
    id VARCHAR(36) NOT NULL,
    enabled BOOLEAN,
    token VARCHAR(100),
    modified_by VARCHAR(36),
    update_at BIGINT,
    workspace_id VARCHAR(36),
    CONSTRAINT {p}sharing_pkey PRIMARY KEY (id)
);

CREATE TABLE {p}subscriptions (
    block_type VARCHAR(10),
    block_id VARCHAR(36) NOT NULL,
    workspace_id VARCHAR(36),
    subscriber_type VARCHAR(10),
    subscriber_id VARCHAR(36) NOT NULL,
    notified_at BIGINT,
    create_at BIGINT,
    delete_at BIGINT,
    CONSTRAINT {p}subscriptions_pkey PRIMARY KEY (block_id, subscriber_id)
);
CREATE INDEX idx_subscriptions_subscriber_id ON {p}subscriptions (subscriber_id);

CREATE TABLE {p}system_settings (
    id VARCHAR(100) NOT NULL,
    value TEXT,
    CONSTRAINT {p}system_settings_pkey PRIMARY KEY (id)
);

CREATE TABLE {p}teams (
    id VARCHAR(36) NOT NULL,
    signup_token VARCHAR(100) NOT NULL,
    settings JSON,
    modified_by VARCHAR(36),
    update_at BIGINT,
    CONSTRAINT {p}workspaces_pkey PRIMARY KEY (id)
);

CREATE TABLE {p}users (
    id VARCHAR(100) NOT NULL,
    username VARCHAR(100),
    email VARCHAR(255),
    password VARCHAR(100),
    mfa_secret VARCHAR(100),
    auth_service VARCHAR(20),
    auth_data VARCHAR(255),
    props JSON,
    create_at BIGINT,
    update_at BIGINT,
    delete_at BIGINT,
    CONSTRAINT {p}users_pkey PRIMARY KEY (id)
);
