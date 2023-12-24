-- Make sure to run `cargo sqlx prepare --workspace` upon updating this and push the .sqlx folder
-- More info: https://github.com/launchbadge/sqlx/blob/main/sqlx-cli/README.md#enable-building-in-offline-mode-with-query

\c mydatabase;

-- Enable UUID extension
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

-- Drop existing tables
DROP TABLE IF EXISTS assistants;
DROP TABLE IF EXISTS threads;
DROP TABLE IF EXISTS messages;
DROP TABLE IF EXISTS runs;

-- Create assistants table
CREATE TABLE assistants (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    object TEXT,
    created_at INTEGER NOT NULL DEFAULT (EXTRACT(EPOCH FROM NOW())),
    name TEXT,
    description TEXT,
    model TEXT,
    instructions TEXT,
    tools JSONB[],
    file_ids TEXT[],
    metadata JSONB,
    user_id UUID
);

-- Create threads table
CREATE TABLE threads (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id UUID,
    file_ids TEXT[],
    object TEXT,
    created_at INTEGER NOT NULL DEFAULT (EXTRACT(EPOCH FROM NOW())),
    metadata JSONB
);

-- Create messages table
CREATE TABLE messages (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    object TEXT,
    created_at INTEGER NOT NULL DEFAULT (EXTRACT(EPOCH FROM NOW())),
    thread_id UUID REFERENCES threads(id),
    role TEXT NOT NULL,
    content JSONB NOT NULL,
    assistant_id UUID REFERENCES assistants(id),
    run_id UUID, -- ! TODO: Change to INTEGER REFERENCES runs(id)
    file_ids TEXT[],
    metadata JSONB,
    user_id UUID
);

-- Create runs table
CREATE TABLE runs (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    object TEXT,
    created_at INTEGER NOT NULL DEFAULT (EXTRACT(EPOCH FROM NOW())),
    thread_id UUID REFERENCES threads(id),
    assistant_id UUID REFERENCES assistants(id),
    status TEXT,
    required_action JSONB,
    last_error JSONB,
    expires_at INTEGER,
    started_at INTEGER,
    cancelled_at INTEGER,
    failed_at INTEGER,
    completed_at INTEGER,
    model TEXT,
    instructions TEXT,
    tools JSONB[],
    file_ids TEXT[],
    metadata JSONB,
    user_id UUID
);

-- Create functions table
CREATE TABLE functions (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id UUID,
    name TEXT UNIQUE, -- ! Is it correct? Meaning the user cannot register the same function name twice
    description TEXT,
    parameters JSONB, -- store as JSON object
    created_at INTEGER NOT NULL DEFAULT (EXTRACT(EPOCH FROM NOW()))
);

-- Create tool_calls table
CREATE TABLE tool_calls (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    output TEXT DEFAULT NULL,
    run_id UUID REFERENCES runs(id),
    created_at INTEGER NOT NULL DEFAULT (EXTRACT(EPOCH FROM NOW())),
    user_id UUID
);