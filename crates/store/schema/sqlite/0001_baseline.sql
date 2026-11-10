-- The baseline schema of a SQLite database. Migrations that have been released are
-- never edited; a change is a new file with the next number.

CREATE TABLE blocks (
    id VARCHAR(36),
    insert_at DATETIME NOT NULL DEFAULT(STRFTIME('%Y-%m-%d %H:%M:%f', 'NOW')),
    parent_id VARCHAR(36),
    schema BIGINT,
    type TEXT,
    title TEXT,
    fields TEXT,
    create_at BIGINT,
    update_at BIGINT,
    delete_at BIGINT,
    root_id VARCHAR(36),
    modified_by VARCHAR(36),
    channel_id VARCHAR(36),
    created_by VARCHAR(36),
    board_id VARCHAR(36),
    PRIMARY KEY (id)
);
CREATE INDEX idx_blocks_board_id_parent_id ON blocks (board_id, parent_id);

CREATE TABLE blocks_history (
    id VARCHAR(36),
    insert_at DATETIME NOT NULL DEFAULT(STRFTIME('%Y-%m-%d %H:%M:%f', 'NOW')),
    parent_id VARCHAR(36),
    schema BIGINT,
    type TEXT,
    title TEXT,
    fields TEXT,
    create_at BIGINT,
    update_at BIGINT,
    delete_at BIGINT,
    root_id VARCHAR(36),
    modified_by VARCHAR(36),
    channel_id VARCHAR(36),
    created_by VARCHAR(36),
    board_id VARCHAR(36),
    PRIMARY KEY (id, insert_at)
);

CREATE TABLE boards (
    id VARCHAR(36) NOT NULL PRIMARY KEY,
    insert_at DATETIME NOT NULL DEFAULT(STRFTIME('%Y-%m-%d %H:%M:%f', 'NOW')),
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
    template_version INT DEFAULT 0,
    properties TEXT,
    card_properties TEXT,
    create_at BIGINT,
    update_at BIGINT,
    delete_at BIGINT,
    minimum_role VARCHAR(36) NOT NULL DEFAULT ''
);
CREATE INDEX idx_boards_team_id_is_template ON boards (team_id, is_template);
CREATE INDEX idx_boards_channel_id ON boards (channel_id);

CREATE TABLE boards_history (
    id VARCHAR(36) NOT NULL,
    insert_at DATETIME NOT NULL DEFAULT(STRFTIME('%Y-%m-%d %H:%M:%f', 'NOW')),
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
    template_version INT DEFAULT 0,
    properties TEXT,
    card_properties TEXT,
    create_at BIGINT,
    update_at BIGINT,
    delete_at BIGINT,
    minimum_role VARCHAR(36) NOT NULL DEFAULT '',
    PRIMARY KEY (id, insert_at)
);

CREATE TABLE board_members (
    board_id VARCHAR(36) NOT NULL,
    user_id VARCHAR(36) NOT NULL,
    roles VARCHAR(64),
    scheme_admin BOOLEAN,
    scheme_editor BOOLEAN,
    scheme_commenter BOOLEAN,
    scheme_viewer BOOLEAN,
    PRIMARY KEY (board_id, user_id)
);
CREATE INDEX idx_board_members_user_id ON board_members (user_id);

CREATE TABLE board_members_history (
    board_id VARCHAR(36) NOT NULL,
    user_id VARCHAR(36) NOT NULL,
    action VARCHAR(10),
    insert_at DATETIME NOT NULL DEFAULT(STRFTIME('%Y-%m-%d %H:%M:%f', 'NOW')),
    PRIMARY KEY (board_id, user_id, insert_at)
);
CREATE INDEX idx_board_members_history_user_id ON board_members_history (user_id);
CREATE INDEX idx_board_members_history_board_id_user_id ON board_members_history (board_id, user_id);

CREATE TABLE categories (
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
    PRIMARY KEY (id)
);
CREATE INDEX idx_categories_user_id_team_id ON categories (user_id, team_id);

CREATE TABLE category_boards (
    id VARCHAR(36) NOT NULL,
    user_id VARCHAR(36) NOT NULL,
    category_id VARCHAR(36) NOT NULL,
    board_id VARCHAR(36) NOT NULL,
    create_at BIGINT,
    update_at BIGINT,
    sort_order BIGINT,
    hidden BOOLEAN,
    PRIMARY KEY (id),
    CONSTRAINT unique_user_category_board UNIQUE (user_id, board_id)
);

CREATE TABLE file_info (
    id VARCHAR(26) NOT NULL,
    create_at BIGINT NOT NULL,
    delete_at BIGINT,
    name TEXT NOT NULL,
    extension VARCHAR(50) NOT NULL,
    size BIGINT NOT NULL,
    archived BOOLEAN,
    path VARCHAR(512)
);

CREATE TABLE notification_hints (
    block_type VARCHAR(10),
    block_id VARCHAR(36),
    workspace_id VARCHAR(36),
    modified_by_id VARCHAR(36),
    create_at BIGINT,
    notify_at BIGINT,
    PRIMARY KEY (block_id)
);

CREATE TABLE preferences (
    userid VARCHAR(36) NOT NULL,
    category VARCHAR(32) NOT NULL,
    name VARCHAR(32) NOT NULL,
    value TEXT NULL,
    PRIMARY KEY (userid, category, name)
);
CREATE INDEX idx_preferences_category ON preferences (category);
CREATE INDEX idx_preferences_name ON preferences (name);

CREATE TABLE sessions (
    id VARCHAR(100),
    token VARCHAR(100),
    user_id VARCHAR(100),
    props TEXT,
    create_at BIGINT,
    update_at BIGINT,
    auth_service VARCHAR(20),
    PRIMARY KEY (id)
);

CREATE TABLE sharing (
    id VARCHAR(36),
    enabled BOOLEAN,
    token VARCHAR(100),
    modified_by VARCHAR(36),
    update_at BIGINT,
    workspace_id VARCHAR(36),
    PRIMARY KEY (id)
);

CREATE TABLE subscriptions (
    block_type VARCHAR(10),
    block_id VARCHAR(36),
    workspace_id VARCHAR(36),
    subscriber_type VARCHAR(10),
    subscriber_id VARCHAR(36),
    notified_at BIGINT,
    create_at BIGINT,
    delete_at BIGINT,
    PRIMARY KEY (block_id, subscriber_id)
);
CREATE INDEX idx_subscriptions_subscriber_id ON subscriptions (subscriber_id);

CREATE TABLE system_settings (
    id VARCHAR(100),
    value TEXT,
    PRIMARY KEY (id)
);

CREATE TABLE teams (
    id VARCHAR(36),
    signup_token VARCHAR(100) NOT NULL,
    settings TEXT,
    modified_by VARCHAR(36),
    update_at BIGINT,
    PRIMARY KEY (id)
);

CREATE TABLE users (
    id VARCHAR(100),
    username VARCHAR(100),
    email VARCHAR(255),
    password VARCHAR(100),
    mfa_secret VARCHAR(100),
    auth_service VARCHAR(20),
    auth_data VARCHAR(255),
    props TEXT,
    create_at BIGINT,
    update_at BIGINT,
    delete_at BIGINT,
    PRIMARY KEY (id)
);
