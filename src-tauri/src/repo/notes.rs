use rusqlite::{params, Connection};
use rusqlite::types::Value;

use crate::error::{err, AppResult};
use crate::models::Note;

use super::{new_id, now_ms};

fn map_note(r: &rusqlite::Row<'_>) -> rusqlite::Result<Note> {
    Ok(Note {
        id: r.get(0)?,
        title: r.get(1)?,
        body: r.get(2)?,
        created_at: r.get(3)?,
        updated_at: r.get(4)?,
    })
}

pub fn list_notes(conn: &Connection, search: Option<&str>) -> AppResult<Vec<Note>> {
    let mut sql = String::from(
        "SELECT id, title, body, created_at, updated_at FROM notes WHERE deleted_at IS NULL",
    );
    let mut vals: Vec<Value> = Vec::new();
    if let Some(s) = search.map(|s| s.trim().to_string()) {
        if !s.is_empty() {
            sql.push_str(" AND (title LIKE ?1 OR body LIKE ?1)");
            vals.push(Value::Text(format!("%{s}%")));
        }
    }
    sql.push_str(" ORDER BY updated_at DESC");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(vals.iter()), map_note)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub fn get_note(conn: &Connection, id: &str) -> AppResult<Note> {
    let mut stmt = conn.prepare(
        "SELECT id, title, body, created_at, updated_at FROM notes WHERE id=?1 AND deleted_at IS NULL",
    )?;
    let mut rows = stmt.query_map([id], map_note)?;
    match rows.next() {
        Some(Ok(n)) => Ok(n),
        _ => err("笔记不存在"),
    }
}

pub fn create_note(conn: &Connection, title: &str, body: &str) -> AppResult<Note> {
    let id = new_id();
    let now = now_ms();
    conn.execute(
        "INSERT INTO notes (id, title, body, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![id, title.trim(), body, now, now],
    )?;
    get_note(conn, &id)
}

pub fn update_note(conn: &Connection, id: &str, title: &str, body: &str) -> AppResult<Note> {
    conn.execute(
        "UPDATE notes SET title=?1, body=?2, updated_at=?3 WHERE id=?4 AND deleted_at IS NULL",
        params![title.trim(), body, now_ms(), id],
    )?;
    get_note(conn, id)
}

pub fn delete_note(conn: &Connection, id: &str) -> AppResult<()> {
    conn.execute("UPDATE notes SET deleted_at=?1 WHERE id=?2", params![now_ms(), id])?;
    Ok(())
}
