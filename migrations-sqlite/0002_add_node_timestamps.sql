ALTER TABLE nodes
    ADD COLUMN created_at INTEGER NOT NULL DEFAULT (unixepoch());

ALTER TABLE nodes
    ADD COLUMN updated_at INTEGER NOT NULL DEFAULT (unixepoch());

CREATE TRIGGER nodes_updated_at_trigger
    AFTER UPDATE ON nodes
    FOR EACH ROW
    BEGIN
        UPDATE nodes
        SET updated_at = unixepoch()
        WHERE public_key = NEW.public_key;
    END;
