\c mydatabase;

-- Drop existing tables
DROP TABLE IF EXISTS assistants;
DROP TABLE IF EXISTS threads;
DROP TABLE IF EXISTS messages;
DROP TABLE IF EXISTS runs;

-- Create assistants table
CREATE TABLE assistants (
    id SERIAL PRIMARY KEY,
    object TEXT,
    created_at BIGINT NOT NULL DEFAULT (EXTRACT(EPOCH FROM NOW()) * 1000),
    name TEXT,
    description TEXT,
    model TEXT,
    instructions TEXT,
    tools TEXT[],
    file_ids TEXT[],
    metadata JSONB,
    user_id TEXT
);

-- Create threads table
CREATE TABLE threads (
    id SERIAL PRIMARY KEY,
    user_id TEXT,
    file_ids TEXT[],
    object TEXT,
    created_at BIGINT NOT NULL DEFAULT (EXTRACT(EPOCH FROM NOW()) * 1000),
    metadata JSONB
);

-- Create messages table
CREATE TABLE messages (
    id SERIAL PRIMARY KEY,
    object TEXT,
    created_at BIGINT NOT NULL DEFAULT (EXTRACT(EPOCH FROM NOW()) * 1000),
    thread_id INTEGER REFERENCES threads(id),
    role TEXT NOT NULL,
    content JSONB NOT NULL,
    assistant_id INTEGER REFERENCES assistants(id),
    run_id TEXT, -- ! TODO: Change to INTEGER REFERENCES runs(id)
    file_ids TEXT[],
    metadata JSONB,
    user_id TEXT
);

-- Create runs table
CREATE TABLE runs (
    id SERIAL PRIMARY KEY,
    object TEXT,
    created_at BIGINT NOT NULL DEFAULT (EXTRACT(EPOCH FROM NOW()) * 1000),
    thread_id INTEGER REFERENCES threads(id),
    assistant_id INTEGER REFERENCES assistants(id),
    status TEXT,
    required_action JSONB,
    last_error JSONB,
    expires_at BIGINT,
    started_at BIGINT,
    cancelled_at BIGINT,
    failed_at BIGINT,
    completed_at BIGINT,
    model TEXT,
    instructions TEXT,
    tools TEXT[],
    file_ids TEXT[],
    metadata JSONB,
    user_id TEXT
);