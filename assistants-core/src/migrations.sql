-- Drop existing tables
DROP TABLE IF EXISTS assistants;
DROP TABLE IF EXISTS threads;
DROP TABLE IF EXISTS messages;
DROP TABLE IF EXISTS runs;

-- Create assistants table
CREATE TABLE assistants (
    id SERIAL PRIMARY KEY,
    instructions TEXT,
    name TEXT,
    tools TEXT[],
    model TEXT,
    user_id TEXT
);

-- Create threads table
CREATE TABLE threads (
    id SERIAL PRIMARY KEY,
    user_id TEXT
);

-- Create messages table
CREATE TABLE messages (
    id SERIAL PRIMARY KEY,
    created_at BIGINT NOT NULL DEFAULT (EXTRACT(EPOCH FROM NOW()) * 1000),
    thread_id TEXT NOT NULL,
    role TEXT NOT NULL,
    content JSONB NOT NULL,
    assistant_id TEXT,
    run_id TEXT,
    file_ids TEXT[],
    metadata JSONB,
    user_id TEXT NOT NULL
);

-- Create runs table
CREATE TABLE runs (
    id SERIAL PRIMARY KEY,
    thread_id TEXT,
    assistant_id TEXT,
    instructions TEXT,
    status TEXT,
    user_id TEXT
);

