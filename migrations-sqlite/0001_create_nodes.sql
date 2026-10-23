CREATE TABLE nodes (
    public_key     TEXT PRIMARY KEY,
    alias          TEXT NOT NULL,
    capacity_sats  INTEGER NOT NULL CHECK (capacity_sats >= 0),
    first_seen     INTEGER NOT NULL,
    rank_position  INTEGER NOT NULL,
    imported_at    INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX nodes_rank_position_idx
    ON nodes (rank_position);
