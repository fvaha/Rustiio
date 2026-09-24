//! Shema baze i migracije. SQL je na jednom mjestu — lakše ga je čitati i mijenjati.
//!
//! Pravilo: **stabilni id-evi**. `items.id` se dodjeljuje jednom i ne mijenja se
//! pri ponovnom skeniranju (upsert po `path`), jer watch-state i "nastavi gledati"
//! vise o tom id-u.

use rusqlite::Connection;

/// Verzija sheme; raste sa svakom izmjenom tablica (`PRAGMA user_version`).
pub const VERSION: i32 = 2;

/// v1 → v2: dva stupca za poster (datoteka u kešu + odakle je došao).
const MIGRATION_V2: &str = r#"
ALTER TABLE items ADD COLUMN poster        TEXT;
ALTER TABLE items ADD COLUMN poster_source TEXT;
"#;

const SCHEMA_V1: &str = r#"
-- Mape koje skeniramo (labela + putanja + vrsta).
CREATE TABLE IF NOT EXISTS roots (
    id      INTEGER PRIMARY KEY,
    label   TEXT    NOT NULL,
    path    TEXT    NOT NULL UNIQUE,
    kind    TEXT    NOT NULL
);

-- Sve što je na disku: mape, video, audio, slike.
CREATE TABLE IF NOT EXISTS items (
    id            INTEGER PRIMARY KEY,
    root_id       INTEGER NOT NULL REFERENCES roots(id) ON DELETE CASCADE,
    parent_id     INTEGER REFERENCES items(id) ON DELETE CASCADE,
    path          TEXT    NOT NULL UNIQUE,
    kind          TEXT    NOT NULL,
    title         TEXT    NOT NULL,
    ext           TEXT    NOT NULL DEFAULT '',
    size          INTEGER NOT NULL DEFAULT 0,
    mtime         INTEGER NOT NULL DEFAULT 0,
    added_at      INTEGER NOT NULL DEFAULT 0,
    -- ffprobe (NULL dok se ne izmjeri)
    duration_ms   INTEGER,
    width         INTEGER,
    height        INTEGER,
    video_codec   TEXT,
    audio_codec   TEXT,
    audio_channels INTEGER,
    bitrate       INTEGER,
    probed_at     INTEGER,
    -- serije (iz imena datoteke)
    series        TEXT,
    season        INTEGER,
    episode       INTEGER
);

CREATE INDEX IF NOT EXISTS items_parent ON items(parent_id);
CREATE INDEX IF NOT EXISTS items_kind   ON items(kind);
CREATE INDEX IF NOT EXISTS items_series ON items(series, season, episode);

-- Pretraga po naslovu/putanji (FTS5, bez dijakritike za hrvatska slova).
CREATE VIRTUAL TABLE IF NOT EXISTS items_fts USING fts5(
    title,
    path,
    content='items',
    content_rowid='id',
    tokenize="unicode61 remove_diacritics 2"
);

CREATE TRIGGER IF NOT EXISTS items_fts_insert AFTER INSERT ON items BEGIN
    INSERT INTO items_fts(rowid, title, path) VALUES (new.id, new.title, new.path);
END;

CREATE TRIGGER IF NOT EXISTS items_fts_delete AFTER DELETE ON items BEGIN
    INSERT INTO items_fts(items_fts, rowid, title, path) VALUES ('delete', old.id, old.title, old.path);
END;

CREATE TRIGGER IF NOT EXISTS items_fts_update AFTER UPDATE OF title, path ON items BEGIN
    INSERT INTO items_fts(items_fts, rowid, title, path) VALUES ('delete', old.id, old.title, old.path);
    INSERT INTO items_fts(rowid, title, path) VALUES (new.id, new.title, new.path);
END;

-- Gdje je koji uređaj stao (ključ je UDN, a ako ga nema — User-Agent).
CREATE TABLE IF NOT EXISTS play_state (
    device      TEXT    NOT NULL,
    item_id     INTEGER NOT NULL REFERENCES items(id) ON DELETE CASCADE,
    position_ms INTEGER NOT NULL DEFAULT 0,
    duration_ms INTEGER,
    played      INTEGER NOT NULL DEFAULT 0,
    updated_at  INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (device, item_id)
);

CREATE INDEX IF NOT EXISTS play_state_recent ON play_state(device, updated_at DESC);

-- Uređaji koje smo vidjeli (capture: što su tražili, koji profil im je dodijeljen).
CREATE TABLE IF NOT EXISTS devices (
    key           TEXT PRIMARY KEY,
    user_agent    TEXT NOT NULL DEFAULT '',
    friendly_name TEXT NOT NULL DEFAULT '',
    profile_id    TEXT NOT NULL DEFAULT '',
    first_seen    INTEGER NOT NULL DEFAULT 0,
    last_seen     INTEGER NOT NULL DEFAULT 0,
    requests      INTEGER NOT NULL DEFAULT 0,
    streams       INTEGER NOT NULL DEFAULT 0
);
"#;

/// Otvori (ili napravi) bazu i primijeni migracije. Idempotentno.
pub fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    // WAL: čitanje ne blokira pisanje tijekom skena.
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;

    let current: i32 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if current >= VERSION {
        return Ok(());
    }

    if current < 1 {
        conn.execute_batch(SCHEMA_V1)?;
    }

    // v2: poster koji je server našao/dohvatio (`art/<id>.<ext>` u config mapi).
    if current < 2 {
        conn.execute_batch(MIGRATION_V2)?;
    }

    conn.pragma_update(None, "user_version", VERSION)?;
    Ok(())
}
