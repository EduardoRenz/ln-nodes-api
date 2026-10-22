ALTER TABLE nodes
    ADD COLUMN IF NOT EXISTS created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    ADD COLUMN IF NOT EXISTS updated_at TIMESTAMPTZ NOT NULL DEFAULT now();

CREATE OR REPLACE FUNCTION set_nodes_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = now();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS nodes_updated_at_trigger ON nodes;

CREATE TRIGGER nodes_updated_at_trigger
    BEFORE UPDATE ON nodes
    FOR EACH ROW
    EXECUTE FUNCTION set_nodes_updated_at();
