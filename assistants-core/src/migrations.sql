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
    thread_id TEXT,
    role TEXT,
    content TEXT,
    user_id TEXT
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

