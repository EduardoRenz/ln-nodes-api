CREATE TABLE nodes (
    public_key     TEXT PRIMARY KEY,
    alias          TEXT NOT NULL,
    capacity_sats  BIGINT NOT NULL CHECK (capacity_sats >= 0),
    first_seen     TIMESTAMPTZ NOT NULL,
    rank_position  INTEGER NOT NULL,
    imported_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX nodes_rank_position_idx
    ON nodes (rank_position);
